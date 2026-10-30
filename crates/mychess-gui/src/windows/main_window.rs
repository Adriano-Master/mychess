//! The main window: menu bar, panel grid, status bar and the About box.

use iced::widget::{button, column, container, pane_grid, stack, text};
use iced::{Element, Length};

use super::{
    analysis, annotate_dialog, book_panel, browser, dialogs, game_list, graph, menu_bar, notation,
    notation_panel, online_panel, pane_tabs, panes, report_panel, ribbon, status_bar,
    tablebase_panel, tournament_panel, tree,
};
use super::{crosstable_panel, epd, filter_stats_panel, ptracker_panel, rating_panel};
use crate::app_api;
use crate::keymap::Platform;
use crate::shell::{Message, Shell, UiMessage};
use crate::theme;
use crate::windows::layout::PaneKind;

/// Space between the panels and around the grid (D214).
const PANE_SPACING: f32 = 6.0;
/// Margin between the grid and the window edge.
const WINDOW_MARGIN: f32 = 8.0;

fn grid(shell: &Shell) -> Element<'_, Message> {
    let bar_owner = shell.ui.command_bar_owner();
    pane_grid(&shell.ui.panes, |pane, group, _maximized| {
        let kind = group.active_kind();
        // The command bar goes under the selected tab of one group (D1002).
        let with_bar = bar_owner == Some(pane);
        let body = match kind {
            PaneKind::Info | PaneKind::Notation | PaneKind::Comment => {
                notation_panel::view(shell, kind, with_bar)
            }
            PaneKind::GameList => bar_if(shell, with_bar, game_list::view(shell), "pane-bar-games"),
            PaneKind::Analysis => {
                bar_if(shell, with_bar, analysis::view(shell), "pane-bar-analysis")
            }
            PaneKind::Graph => bar_if(shell, with_bar, graph::view(shell), "pane-bar-graph"),
            PaneKind::Tree => bar_if(shell, with_bar, tree::view(shell), "pane-bar-tree"),
            PaneKind::Report => bar_if(
                shell,
                with_bar,
                report_panel::view(shell),
                "pane-bar-report",
            ),
            PaneKind::Book => bar_if(shell, with_bar, book_panel::view(shell), "pane-bar-book"),
            PaneKind::Tablebase => bar_if(
                shell,
                with_bar,
                tablebase_panel::view(shell),
                "pane-bar-tablebase",
            ),
            PaneKind::Crosstable => bar_if(
                shell,
                with_bar,
                crosstable_panel::view(shell),
                "pane-bar-crosstable",
            ),
            PaneKind::FilterStats => bar_if(
                shell,
                with_bar,
                filter_stats_panel::view(shell),
                "pane-bar-filter-stats",
            ),
            PaneKind::RatingGraph => bar_if(
                shell,
                with_bar,
                rating_panel::view(shell),
                "pane-bar-rating-graph",
            ),
            PaneKind::PieceTracker => bar_if(
                shell,
                with_bar,
                ptracker_panel::view(shell),
                "pane-bar-piece-tracker",
            ),
            PaneKind::Epd => bar_if(shell, with_bar, epd::view(shell), "pane-bar-epd"),
            PaneKind::Tournament => bar_if(
                shell,
                with_bar,
                tournament_panel::view(shell),
                "pane-bar-tournament",
            ),
            _ => panes::board_pane(shell),
        };
        pane_grid::Content::new(body)
            .title_bar(pane_tabs::title_bar(shell, group))
            .style(theme::pane_content)
    })
    .style(theme::pane_grid)
    .spacing(PANE_SPACING)
    .min_size(if shell.ui.panes.len() > 1 { 160 } else { 240 })
    .on_drag(|event| Message::Ui(UiMessage::PaneDragged(event)))
    .on_resize(8, |event| Message::Ui(UiMessage::PaneResized(event)))
    .into()
}

/// `body`, with the command bar under it when this group owns the bar.
fn bar_if<'a>(
    shell: &'a Shell,
    with_bar: bool,
    body: Element<'a, Message>,
    id: &'static str,
) -> Element<'a, Message> {
    if with_bar {
        notation_panel::under_bar(shell, body, id)
    } else {
        body
    }
}

fn about(shell: &Shell) -> Element<'_, Message> {
    let i18n = &shell.i18n;
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("version", env!("CARGO_PKG_VERSION"));
    let card = container(
        column![
            text(i18n.tr("about-title")).size(20),
            text(i18n.tr_args("about-version", &args)),
            text(i18n.tr("about-description")),
            text(i18n.tr("about-license")),
            text(i18n.tr("about-credits")),
            button(text(i18n.tr("dialog-ok"))).on_press(Message::Ui(UiMessage::ShowAbout(false))),
        ]
        .spacing(10)
        .max_width(480),
    )
    .id("about-box")
    .padding(20)
    .style(theme::pane_body);
    container(card)
        .center(Length::Fill)
        .style(theme::scrim)
        .into()
}

/// The text on the left of the status bar and whether it is an alert:
/// the move being typed, else the newest remark (until it expires), else
/// "Ready" (spec M2-4, D204).
fn status_text(shell: &Shell) -> (String, bool) {
    let i18n = &shell.i18n;
    let entry = shell.app.session.move_entry();
    if !entry.text().is_empty() {
        return (status_bar::entry_text(i18n, entry), false);
    }
    if !shell.ui.dialogs.status.expired() {
        if let Some(notice) = &shell.ui.status_notice {
            // The remarks of the tournament name participants.
            if let mychess_app::AppNotice::Tournament(notice) = notice {
                let names = &shell.ui.dialogs.tournament.snapshot.names;
                return (
                    crate::windows::tournament_text::notice_text(i18n, notice, names),
                    true,
                );
            }
            return (status_bar::app_notice_text(i18n, notice), true);
        }
        if let Some(notice) = app_api::notice(&shell.app) {
            return (status_bar::notice_text(i18n, notice), true);
        }
    }
    if let Some(key) = shell.ui.dialogs.play.note
        && !shell.ui.dialogs.status.expired()
    {
        return (i18n.tr(key), true);
    }
    // A position is being drawn for a tactical game (M8-7, D2460).
    if shell.app.play_picking() {
        return (i18n.tr("play-status-picking"), false);
    }
    // The engine of a game is being started (D1484).
    if shell.app.play_live() && app_api::play_phase(&shell.app).is_none() {
        return (i18n.tr("play-status-starting"), false);
    }
    let text = app_api::prefs_remark(&shell.app).map_or_else(
        || i18n.tr("status-ready"),
        |issue| status_bar::prefs_issue_text(i18n, issue),
    );
    (text, false)
}

/// What the status bar shows in place of the opening of a base, with its
/// stop button (spec M4-6, M8-7).
fn opening_bar(shell: &Shell) -> Option<status_bar::OpeningBar> {
    // The progress of an annotation takes the place of the opening of
    // a base (spec M4-6): the game cannot be edited meanwhile.
    shell
        .app
        .annotation_progress()
        .map(|progress| status_bar::OpeningBar {
            text: if shell.app.report_running() {
                report_panel::progress_text(&shell.i18n, &progress, shell.app.report_eta_seconds())
            } else {
                annotate_dialog::progress_text(&shell.i18n, &progress)
            },
            fraction: annotate_dialog::progress_fraction(&progress),
            cancel_label: shell.i18n.tr("annotate-stop"),
            cancel: Message::App(mychess_app::Message::Annotate(
                mychess_app::AnnotateMessage::Cancel,
            )),
        })
        .or_else(|| tournament_panel::status_progress(shell))
        .or_else(|| {
            app_api::base_opening(&shell.app).map(|(name, fraction)| status_bar::OpeningBar {
                text: status_bar::opening_text(&shell.i18n, &name, fraction),
                fraction,
                cancel_label: shell.i18n.tr("dialog-cancel"),
                cancel: Message::App(app_api::cancel_open()),
            })
        })
        .or_else(|| {
            // The drawing of a position for a tactical game can be given
            // up (M8-7, D2460, D2471): "Quit" with no game on only
            // cancels the drawing.
            shell.app.play_picking().then(|| status_bar::OpeningBar {
                text: shell.i18n.tr("play-status-picking"),
                fraction: None,
                cancel_label: shell.i18n.tr("dialog-cancel"),
                cancel: Message::PlayUi(crate::windows::play_dialog::PlayUiMessage::Quit),
            })
        })
}

/// The whole window.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let specs = menu_bar::menu_specs(&shell.app, &shell.ui, shell.options);
    let ribbon_style = app_api::menu_style(&shell.app) == mychess_app::MenuStylePref::Ribbon;
    let collapsed = app_api::ribbon_collapsed(&shell.app);
    let platform = Platform::current();
    let flyout = ribbon_style
        .then(|| ribbon::flyout(&specs, &shell.ui.ribbon, collapsed, &shell.i18n, platform))
        .flatten();
    let menus = if ribbon_style {
        ribbon::view(&specs, &shell.ui.ribbon, collapsed, &shell.i18n, platform)
    } else {
        menu_bar::view(specs, &shell.i18n, platform)
    };
    let mut page = column![
        menus,
        container(grid(shell))
            .padding(WINDOW_MARGIN)
            .height(Length::Fill),
    ];
    // The assistance is off during an online game: always visible (D1330).
    if let Some(banner) = online_panel::lock_banner(&shell.app, &shell.i18n) {
        page = page.push(banner);
    }
    // The window of the patterns, hidden to show the board (spec M6-2d).
    if let Some(bar) = super::pattern_search_dialog::bar(shell) {
        page = page.push(bar);
    }
    if shell.options.show_status_bar {
        let (left, alert) = status_text(shell);
        let opening = opening_bar(shell);
        page = page.push(status_bar::view(
            left,
            alert,
            opening,
            app_api::board_view(&shell.app).position,
            app_api::is_modified(&shell.app),
            [
                status_bar::variant_badge(&shell.i18n, shell.app.session.game()),
                (shell.app.eboard().phase() == mychess_app::eboard::EboardPhase::Connected)
                    .then(|| shell.i18n.tr("eboard-status-connected")),
            ]
            .into_iter()
            .flatten()
            .collect(),
            &shell.i18n,
        ));
    }
    let mut layers = stack![page];
    // The collapsed ribbon showing its groups floats over the page.
    if let Some(floating) = flyout {
        layers = layers.push(floating);
    }
    if shell.ui.show_about {
        layers = layers.push(about(shell));
    }
    // The browser is below the questions it may raise (loading a game
    // over an altered one asks first).
    if let Some(browser) = browser::view(shell) {
        layers = layers.push(browser);
    }
    // The opening report window (spec M7-2c) is below the questions and
    // dialogs it may raise (merge, confirmations).
    if let Some(report) = super::opening_report::layer(shell) {
        layers = layers.push(report);
    }
    // The player report (spec M7-3c) and its dialog.
    if let Some(report) = super::player_report::layer(shell) {
        layers = layers.push(report);
    }
    // The window of the board image (spec M7-4c).
    if let Some(card) = super::export_image::layer(shell) {
        layers = layers.push(card);
    }
    // The manual (spec M9-5) is above the report and below the dialogs.
    // The maintenance window (spec M7-1c) is at the same level: its own
    // cards and confirmations are part of the layer.
    if let Some(maintenance) = super::maintenance::layer(shell) {
        layers = layers.push(maintenance);
    }
    if let Some(manual) = super::manual::layer(shell) {
        layers = layers.push(manual);
    }
    // The welcome and the tip of the day (spec M9-4).
    if let Some(start) = super::start_dialogs::layer(shell) {
        layers = layers.push(start);
    }
    if let Some(modal) = dialogs::view(shell) {
        layers = layers.push(modal);
    } else if let Some(card) = epd::overlay(shell) {
        // The questions and forms of the EPD window (spec M7-5c).
        layers = layers.push(card);
    } else if let Some(popup) = notation::overlay(shell)
        .or_else(|| game_list::overlay(shell))
        .or_else(|| pane_tabs::menu_overlay(shell))
        .or_else(|| super::mask::context::overlay(shell))
    {
        // The notation's context menu and board preview, unless a dialog
        // owns the window.
        layers = layers.push(popup);
    }
    layers.into()
}
