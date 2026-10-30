//! The panel of the tournament between engines: the toolbar, the progress
//! and the five tabs (Live, Pairings, Standings, Crosstable, Log) (spec
//! M8-2b, D1283, D1360-D1379).
//!
//! The panel is a `PaneKind::Tournament` that can be grouped in tabs with the
//! other panels; closing it does not end the tournament, which lives in the
//! `AppState` (D1361). Everything is drawn from the tournament the
//! application offers and from the snapshot of [`TournamentUi`].

use iced::widget::{
    Column, button, column, container, mouse_area, progress_bar, row, scrollable, space, text,
};
use iced::{Alignment, Element, Length};
use mychess_app::AppState;
use mychess_app::tournament::{
    ExportFormat, ExportTable, GameStatus, Phase, StandingRow, Tournament,
    duration_text as ms_text, elo_text, los_text, margin_text,
};
use mychess_core::{Color, GameResult};

use crate::app_api;
use crate::i18n::I18n;
use crate::shell::Message;
use crate::shell::Shell;
use crate::theme::{self, RowLook};
use crate::widgets::board::BoardWidget;
use crate::windows::annotate_dialog::{choice, group};
use crate::windows::tournament_text::{
    cell_text, clock_text, log_text, name_of, phase_key, reliability_key, status_text,
};
use crate::windows::tournament_ui::{
    LOG_LINES, ROW_HEIGHT, TournamentConfirm, TournamentTab, TournamentUi, TournamentUiMessage,
};

/// Id of the list of games (to scroll it to a game).
pub const PAIRINGS_LIST_ID: &str = "tournament-pairings";
/// Height of the list of games.
/// Side of the board of the live tab.
const BOARD_SIDE: f32 = 360.0;

fn send(message: TournamentUiMessage) -> Message {
    Message::Tournament(message)
}

fn tool<'a>(label: String, message: Option<TournamentUiMessage>) -> Element<'a, Message> {
    button(text(label).size(13))
        .padding([3, 8])
        .style(button::secondary)
        .on_press_maybe(message.map(send))
        .into()
}

fn cell<'a>(content: impl Into<String>, width: Length) -> Element<'a, Message> {
    text(content.into()).size(13).width(width).into()
}

fn points_of(row: &StandingRow) -> String {
    row.points_text()
}

/// Sonneborn-Berger comes in quarter points.
fn sb_text(quarters: u64) -> String {
    format!("{}.{:02}", quarters / 4, (quarters % 4) * 25)
}

// ---- the toolbar ----

fn toolbar<'a>(t: &Tournament, i18n: &I18n) -> Element<'a, Message> {
    let phase = t.phase();
    let mut bar = row![].spacing(6).align_y(Alignment::Center);
    match phase {
        Phase::Running => {
            bar = bar
                .push(tool(
                    i18n.tr("tournament-pause"),
                    Some(TournamentUiMessage::Pause),
                ))
                .push(tool(
                    i18n.tr("tournament-stop-after"),
                    Some(TournamentUiMessage::StopAfter),
                ));
            let live = t.live().is_some();
            for (key, result) in [
                ("tournament-adj-white", GameResult::White),
                ("tournament-adj-draw-result", GameResult::Draw),
                ("tournament-adj-black", GameResult::Black),
            ] {
                bar = bar.push(tool(
                    i18n.tr(key),
                    live.then_some(TournamentUiMessage::Request(TournamentConfirm::Adjudicate(
                        result,
                    ))),
                ));
            }
            bar = bar.push(tool(
                i18n.tr("tournament-skip-current"),
                live.then_some(TournamentUiMessage::Request(TournamentConfirm::SkipCurrent)),
            ));
        }
        Phase::Pausing => {
            bar = bar.push(text(i18n.tr("tournament-phase-pausing")).size(13));
        }
        Phase::Paused | Phase::StoppedAfterGame => {
            bar = bar.push(tool(
                i18n.tr("tournament-continue"),
                Some(TournamentUiMessage::Continue),
            ));
        }
        Phase::Decision(_) => {
            bar = bar.push(tool(
                i18n.tr("tournament-decision-reopen"),
                Some(TournamentUiMessage::DecisionReopen),
            ));
        }
        Phase::Finished | Phase::Aborted => {
            bar = bar
                .push(tool(
                    i18n.tr("tournament-restart"),
                    Some(TournamentUiMessage::Restart),
                ))
                .push(tool(
                    i18n.tr("tournament-open-pgn"),
                    Some(TournamentUiMessage::OpenPgn),
                ));
        }
    }
    let over = matches!(phase, Phase::Finished | Phase::Aborted);
    if !over {
        bar = bar.push(tool(
            i18n.tr("tournament-abort"),
            Some(TournamentUiMessage::Request(TournamentConfirm::Abort)),
        ));
    }
    bar = bar.push(space::horizontal()).push(tool(
        i18n.tr(if over {
            "tournament-close"
        } else {
            "tournament-leave"
        }),
        Some(TournamentUiMessage::CloseTournament),
    ));
    bar.wrap().into()
}

fn header<'a>(t: &Tournament, names: &[String], i18n: &I18n) -> Element<'a, Message> {
    let (done, total) = t.progress();
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("done", i64::try_from(done).unwrap_or(i64::MAX));
    args.set("total", i64::try_from(total).unwrap_or(i64::MAX));
    #[allow(clippy::cast_precision_loss)] // a progress bar needs no precision
    let fraction = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    let mut page = column![
        row![
            text(t.config().name.clone()).size(16),
            text(i18n.tr(phase_key(t.phase())))
                .size(13)
                .style(theme::accent_text),
            text(i18n.tr_args("tournament-progress", &args))
                .size(13)
                .style(theme::muted_text),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        progress_bar(0.0..=1.0, fraction).girth(6.0),
    ]
    .spacing(4);
    if matches!(t.phase(), Phase::Finished) {
        let winners = t
            .winners()
            .iter()
            .map(|i| name_of(names, *i))
            .collect::<Vec<_>>()
            .join(", ");
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("winners", winners);
        page = page.push(
            text(i18n.tr_args("tournament-summary-winners", &args))
                .size(14)
                .style(theme::accent_text),
        );
    }
    page.into()
}

fn tabs<'a>(ui: &TournamentUi, i18n: &I18n) -> Element<'a, Message> {
    let items: Vec<Element<'a, Message>> = TournamentTab::ALL
        .iter()
        .map(|tab| {
            choice(i18n.tr(tab.label_key()), *tab, ui.tab, true, |tab| {
                send(TournamentUiMessage::Tab(tab))
            })
        })
        .collect();
    group(items)
}

// ---- standings and crosstable ----

fn export_bar<'a>(ui: &TournamentUi, table: ExportTable, i18n: &I18n) -> Element<'a, Message> {
    let formats = [
        ("tournament-export-csv", ExportFormat::Csv),
        ("tournament-export-text", ExportFormat::Text),
        ("tournament-export-markdown", ExportFormat::Markdown),
        ("tournament-export-html", ExportFormat::Html),
    ];
    let mut bar = row![text(i18n.tr("tournament-export")).size(13)]
        .spacing(6)
        .align_y(Alignment::Center);
    for (key, format) in formats {
        bar = bar.push(choice(i18n.tr(key), format, ui.export_format, true, |f| {
            send(TournamentUiMessage::ExportFormat(f))
        }));
    }
    bar = bar
        .push(tool(
            i18n.tr("tournament-export-save"),
            Some(TournamentUiMessage::Save(table)),
        ))
        .push(tool(
            i18n.tr("tournament-export-copy"),
            Some(TournamentUiMessage::Copy(table)),
        ));
    bar.wrap().into()
}

fn standings<'a>(ui: &TournamentUi, names: &[String], i18n: &I18n) -> Element<'a, Message> {
    let fixed = Length::Fixed(44.0);
    let wide = Length::Fixed(64.0);
    let header = container(
        row![
            cell("#", Length::Fixed(32.0)),
            cell(i18n.tr("tournament-col-name"), Length::Fill),
            cell(i18n.tr("tournament-col-played"), fixed),
            cell(i18n.tr("tournament-col-wins"), fixed),
            cell(i18n.tr("tournament-col-draws"), fixed),
            cell(i18n.tr("tournament-col-losses"), fixed),
            cell(i18n.tr("tournament-col-points"), wide),
            cell(i18n.tr("tournament-col-percent"), wide),
            cell(i18n.tr("tournament-col-sb"), wide),
        ]
        .spacing(6),
    )
    .padding([3, 6])
    .style(theme::list_header);
    let mut rows = Column::new();
    for (i, r) in ui.snapshot.standings.iter().enumerate() {
        let mut name = name_of(names, r.participant);
        if r.withdrawn {
            name.push_str("  (");
            name.push_str(&i18n.tr("tournament-col-withdrawn"));
            name.push(')');
        }
        let line = row![
            cell(r.rank.to_string(), Length::Fixed(32.0)),
            cell(name, Length::Fill),
            cell(r.played.to_string(), fixed),
            cell(r.wins.to_string(), fixed),
            cell(r.draws.to_string(), fixed),
            cell(r.losses.to_string(), fixed),
            cell(points_of(r), wide),
            cell(format!("{:.1}", r.percent()), wide),
            cell(sb_text(r.sonneborn_berger), wide),
        ]
        .spacing(6);
        rows = rows.push(
            container(line)
                .padding([3, 6])
                .style(theme::list_row(RowLook {
                    zebra: i % 2 == 1,
                    dimmed: r.withdrawn,
                    ..RowLook::default()
                })),
        );
    }
    column![
        header,
        scrollable(rows).height(Length::Fill),
        export_bar(ui, ExportTable::Standings, i18n),
    ]
    .spacing(6)
    .into()
}

// ---- ratings and time (M8-2c) ----

fn ratings<'a>(ui: &TournamentUi, names: &[String], i18n: &I18n) -> Element<'a, Message> {
    let fixed = Length::Fixed(44.0);
    let wide = Length::Fixed(64.0);
    let header = container(
        row![
            cell("#", Length::Fixed(32.0)),
            cell(i18n.tr("tournament-col-name"), Length::Fill),
            cell(i18n.tr("tournament-col-played"), fixed),
            cell(i18n.tr("tournament-col-wins"), fixed),
            cell(i18n.tr("tournament-col-draws"), fixed),
            cell(i18n.tr("tournament-col-losses"), fixed),
            cell(i18n.tr("tournament-col-percent"), wide),
            cell(i18n.tr("tournament-col-elo"), wide),
            cell(i18n.tr("tournament-col-margin"), wide),
            cell(i18n.tr("tournament-col-los"), wide),
            cell(i18n.tr("tournament-col-performance"), Length::Fixed(88.0)),
            cell(i18n.tr("tournament-col-sample"), Length::Fixed(120.0)),
        ]
        .spacing(6),
    )
    .padding([3, 6])
    .style(theme::list_header);
    let mut rows = Column::new();
    for (i, r) in ui.snapshot.ratings.iter().enumerate() {
        let e = &r.estimate;
        let mut name = name_of(names, r.participant);
        if r.withdrawn {
            name.push_str("  (");
            name.push_str(&i18n.tr("tournament-col-withdrawn"));
            name.push(')');
        }
        let line = row![
            cell(r.rank.to_string(), Length::Fixed(32.0)),
            cell(name, Length::Fill),
            cell(e.record.games().to_string(), fixed),
            cell(e.record.wins.to_string(), fixed),
            cell(e.record.draws.to_string(), fixed),
            cell(e.record.losses.to_string(), fixed),
            cell(
                e.score
                    .map_or_else(|| "-".to_owned(), |s| format!("{:.1}", s * 100.0)),
                wide
            ),
            cell(elo_text(e.elo), wide),
            cell(margin_text(e.margin()), wide),
            cell(los_text(e.los), wide),
            cell(
                r.performance
                    .map_or_else(|| "-".to_owned(), |p| format!("{p:.0}")),
                Length::Fixed(88.0)
            ),
            cell(
                i18n.tr(reliability_key(e.reliability)),
                Length::Fixed(120.0)
            ),
        ]
        .spacing(6);
        rows = rows.push(
            container(line)
                .padding([3, 6])
                .style(theme::list_row(RowLook {
                    zebra: i % 2 == 1,
                    dimmed: r.withdrawn,
                    ..RowLook::default()
                })),
        );
    }
    column![
        header,
        scrollable(rows).height(Length::Fill),
        text(i18n.tr("tournament-ratings-note"))
            .size(12)
            .style(theme::muted_text),
        export_bar(ui, ExportTable::Ratings, i18n),
    ]
    .spacing(6)
    .into()
}

fn time_tab<'a>(ui: &TournamentUi, names: &[String], i18n: &I18n) -> Element<'a, Message> {
    let fixed = Length::Fixed(64.0);
    let wide = Length::Fixed(84.0);
    let header = container(
        row![
            cell(i18n.tr("tournament-col-name"), Length::Fill),
            cell(i18n.tr("tournament-col-moves"), fixed),
            cell(i18n.tr("tournament-col-mean"), wide),
            cell(i18n.tr("tournament-col-slowest"), wide),
            cell(i18n.tr("tournament-col-overhead"), wide),
            cell(i18n.tr("tournament-col-overruns"), fixed),
            cell(i18n.tr("tournament-col-forfeits"), fixed),
            cell(i18n.tr("tournament-col-clock-used"), wide),
            cell(i18n.tr("tournament-col-verdict"), Length::Fixed(120.0)),
        ]
        .spacing(6),
    )
    .padding([3, 6])
    .style(theme::list_header);
    let mut rows = Column::new();
    for (i, r) in ui.snapshot.time.iter().enumerate() {
        let line = row![
            cell(name_of(names, r.participant), Length::Fill),
            cell(r.stats.moves.to_string(), fixed),
            cell(ms_text(r.mean_ms), wide),
            cell(ms_text((r.stats.moves > 0).then_some(r.stats.max_ms)), wide),
            cell(ms_text(r.overhead_ms), wide),
            cell(r.stats.overruns.to_string(), fixed),
            cell(r.stats.forfeits.to_string(), fixed),
            cell(
                r.clock_used_percent
                    .map_or_else(|| "-".to_owned(), |p| format!("{p:.1}")),
                wide
            ),
            cell(i18n.tr(r.verdict.label_key()), Length::Fixed(120.0)),
        ]
        .spacing(6);
        rows = rows.push(
            container(line)
                .padding([3, 6])
                .style(theme::list_row(RowLook {
                    zebra: i % 2 == 1,
                    ..RowLook::default()
                })),
        );
    }
    column![
        header,
        scrollable(rows).height(Length::Fill),
        text(i18n.tr("tournament-time-note"))
            .size(12)
            .style(theme::muted_text),
        export_bar(ui, ExportTable::Time, i18n),
    ]
    .spacing(6)
    .into()
}

fn cross<'a>(ui: &TournamentUi, names: &[String], i18n: &I18n) -> Element<'a, Message> {
    let n = ui.snapshot.cross.len();
    let side = Length::Fixed(54.0);
    let mut head = row![cell(String::new(), Length::Fixed(150.0))].spacing(4);
    for j in 0..n {
        head = head.push(cell((j + 1).to_string(), side));
    }
    let mut rows = Column::new().spacing(2);
    for (i, line) in ui.snapshot.cross.iter().enumerate() {
        let mut r = row![
            text(format!("{}. {}", i + 1, name_of(names, i)))
                .size(13)
                .width(Length::Fixed(150.0))
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        for (j, games) in line.iter().enumerate() {
            let label: String = if i == j {
                "-".to_owned()
            } else {
                games
                    .iter()
                    .map(|(_, score)| cell_text(*score))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let clickable = i != j && !games.is_empty();
            r = r.push(
                button(text(label).size(13))
                    .padding([2, 4])
                    .width(side)
                    .style(button::text)
                    .on_press_maybe(clickable.then(|| send(TournamentUiMessage::Cell(i, j)))),
            );
        }
        rows = rows.push(r);
    }
    column![
        scrollable(column![head, rows].spacing(4))
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .height(Length::Fill)
            .width(Length::Fill),
        export_bar(ui, ExportTable::Cross, i18n),
    ]
    .spacing(6)
    .into()
}

// ---- pairings ----

fn pairings<'a>(
    ui: &'a TournamentUi,
    t: &'a Tournament,
    names: &'a [String],
    i18n: &'a I18n,
) -> Element<'a, Message> {
    // The list fills the panel; the rows built follow the height it got.
    iced::widget::responsive(move |size| pairings_list(ui, t, names, i18n, size.height)).into()
}

fn pairings_list<'a>(
    ui: &TournamentUi,
    t: &Tournament,
    names: &[String],
    i18n: &I18n,
    height: f32,
) -> Element<'a, Message> {
    let slots = t.slots();
    let total = slots.len();
    let (first, count) = ui.window_for(total, height);
    let spacer = |rows: usize| {
        #[allow(clippy::cast_precision_loss)] // a few thousand rows at most
        let height = rows as f32 * ROW_HEIGHT;
        space::vertical().height(Length::Fixed(height))
    };
    let live = matches!(
        t.phase(),
        Phase::Paused | Phase::StoppedAfterGame | Phase::Running
    );
    let mut list = column![spacer(first)];
    for slot in slots.iter().skip(first).take(count) {
        let p = &slot.pairing;
        let selected = ui.selected == Some(p.number);
        let mut line = row![
            cell(p.number.to_string(), Length::Fixed(36.0)),
            cell(p.round_label(), Length::Fixed(48.0)),
            cell(name_of(names, p.white), Length::FillPortion(3)),
            cell(name_of(names, p.black), Length::FillPortion(3)),
            cell(status_text(i18n, &slot.status), Length::FillPortion(3)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        match &slot.status {
            GameStatus::Pending if live => {
                line = line
                    .push(tool(
                        i18n.tr("tournament-to-front"),
                        Some(TournamentUiMessage::ToFront(p.number)),
                    ))
                    .push(tool(
                        i18n.tr("tournament-skip"),
                        Some(TournamentUiMessage::SkipGame(p.number)),
                    ));
            }
            GameStatus::Played(_) | GameStatus::Skipped if live => {
                line = line.push(tool(
                    i18n.tr("tournament-redo"),
                    Some(TournamentUiMessage::Redo(p.number)),
                ));
            }
            _ => {}
        }
        list = list.push(
            mouse_area(
                container(line)
                    .height(Length::Fixed(ROW_HEIGHT))
                    .padding([0, 6])
                    .align_y(Alignment::Center)
                    .style(theme::list_row(RowLook {
                        zebra: p.number % 2 == 0,
                        selected,
                        ..RowLook::default()
                    })),
            )
            .on_press(send(TournamentUiMessage::Select(p.number))),
        );
    }
    list = list.push(spacer(total.saturating_sub(first + count)));
    scrollable(list)
        .id(PAIRINGS_LIST_ID)
        .height(Length::Fill)
        .on_scroll(|viewport| send(TournamentUiMessage::Scrolled(viewport.absolute_offset().y)))
        .into()
}

// ---- live ----

fn live_tab<'a>(shell: &'a Shell, t: &Tournament, names: &[String]) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let ui = &shell.ui.dialogs.tournament;
    let Some(live) = t.live() else {
        return text(i18n.tr("tournament-live-none")).into();
    };
    let appearance = app_api::appearance(&shell.app, shell.options);
    let board: Element<'a, Message> =
        match ui.board_data(&shell.app, shell.ui.pieces(), appearance.board) {
            Some(data) => {
                let (caches, _) = ui.board_parts();
                container(
                    BoardWidget::new(data, caches)
                        .into_element()
                        .map(|_| send(TournamentUiMessage::Noop)),
                )
                .width(Length::Fixed(BOARD_SIDE))
                .height(Length::Fixed(BOARD_SIDE))
                .into()
            }
            None => container(text(i18n.tr("tournament-live-preparing")))
                .center(Length::Fixed(BOARD_SIDE))
                .into(),
        };
    let flipped = TournamentUi::orientation(&shell.app)
        == crate::widgets::board::geometry::Orientation::Black;
    let (top, bottom) = if flipped {
        (Color::White, Color::Black)
    } else {
        (Color::Black, Color::White)
    };
    let player = |side: Color| {
        let index = if side == Color::White {
            live.white
        } else {
            live.black
        };
        let clock = live
            .clock
            .map_or_else(String::new, |c| clock_text(c.shown(side, ui.now)));
        row![
            text(name_of(names, index)).size(15).width(Length::Fill),
            text(clock).size(18),
        ]
        .align_y(Alignment::Center)
    };
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("number", i64::from(live.number));
    args.set("attempt", i64::from(live.attempt) + 1);
    let mut moves = Column::new().spacing(2);
    for line in &ui.snapshot.live.lines {
        moves = moves.push(text(line.as_str()).size(13));
    }
    // With several games at once, one button per game (D2508).
    let mut picker = row![].spacing(6).align_y(Alignment::Center);
    if ui.snapshot.lanes.len() > 1 {
        for lane in &ui.snapshot.lanes {
            let label = format!(
                "#{} {} - {}",
                lane.number,
                name_of(names, lane.white),
                name_of(names, lane.black)
            );
            let watched = ui.snapshot.lanes.iter().find(|l| l.watched).map(|l| l.lane);
            picker = picker.push(choice(
                label,
                lane.lane,
                watched.unwrap_or(live.lane),
                true,
                |l| send(TournamentUiMessage::Watch(l)),
            ));
        }
    }
    let side = column![
        text(i18n.tr_args("tournament-live-game", &args))
            .size(13)
            .style(theme::muted_text),
        picker.wrap(),
        player(top),
        scrollable(moves).height(Length::Fill),
        player(bottom),
    ]
    .spacing(8)
    .width(Length::Fill);
    row![board, side].spacing(12).height(Length::Fill).into()
}

// ---- log ----

fn log_tab<'a>(t: &Tournament, names: &[String], i18n: &I18n) -> Element<'a, Message> {
    let log = t.log();
    let skip = log.len().saturating_sub(LOG_LINES);
    let mut lines = Column::new().spacing(2);
    for event in log.iter().skip(skip) {
        lines = lines.push(text(log_text(i18n, event, names)).size(13));
    }
    scrollable(lines)
        .anchor_bottom()
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

// ---- the panel ----

fn empty<'a>(i18n: &I18n) -> Element<'a, Message> {
    container(
        column![
            text(i18n.tr("tournament-none")).size(14),
            row![
                tool(i18n.tr("tournament-new"), Some(TournamentUiMessage::Open)),
                tool(
                    i18n.tr("tournament-resume"),
                    Some(TournamentUiMessage::OpenResume),
                ),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .center(Length::Fill)
    .into()
}

/// The panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let i18n = &shell.i18n;
    let app: &AppState = &shell.app;
    let ui = &shell.ui.dialogs.tournament;
    let Some(t) = app.tournament() else {
        return container(empty(i18n))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::pane_body)
            .into();
    };
    let names = &ui.snapshot.names;
    let body: Element<'_, Message> = match ui.tab {
        TournamentTab::Standings => standings(ui, names, i18n),
        TournamentTab::Cross => cross(ui, names, i18n),
        TournamentTab::Pairings => pairings(ui, t, names, i18n),
        TournamentTab::Live => live_tab(shell, t, names),
        TournamentTab::Log => log_tab(t, names, i18n),
        TournamentTab::Ratings => ratings(ui, names, i18n),
        TournamentTab::Time => time_tab(ui, names, i18n),
    };
    container(
        column![
            header(t, names, i18n),
            toolbar(t, i18n),
            tabs(ui, i18n),
            container(body).height(Length::Fill),
        ]
        .spacing(8),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::pane_body)
    .into()
}

/// The progress of a tournament on, for the status bar: the games played,
/// the bar and a button that asks to end it (D1365).
pub fn status_progress(shell: &Shell) -> Option<crate::windows::status_bar::OpeningBar> {
    let t = shell.app.tournament()?;
    if !shell.app.tournament_live() {
        return None;
    }
    let (done, total) = t.progress();
    let i18n = &shell.i18n;
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("name", t.config().name.clone());
    args.set("done", i64::try_from(done).unwrap_or(i64::MAX));
    args.set("total", i64::try_from(total).unwrap_or(i64::MAX));
    #[allow(clippy::cast_precision_loss)] // a progress bar needs no precision
    let fraction = (total > 0).then(|| done as f32 / total as f32);
    Some(crate::windows::status_bar::OpeningBar {
        text: i18n.tr_args("tournament-status-progress", &args),
        fraction,
        cancel_label: i18n.tr("tournament-abort"),
        cancel: send(TournamentUiMessage::Request(TournamentConfirm::Abort)),
    })
}
