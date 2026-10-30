//! The panels of the main window. Notation and information are provisional
//! (the notation and game-information slices replace them).

use fluent_bundle::FluentArgs;
use iced::widget::container;
use iced::{Element, Length};
use mychess_core::Tags;

use crate::app_api;
use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme;
use crate::widgets::board::premove::PremoveSetup;
use crate::windows::board_column::{BoardColumn, SideBars};
use crate::windows::{online_panel, play_panel};

/// The three lines of the information panel, already formatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderView {
    /// `White (elo) - Black (elo)  result`.
    pub players: String,
    /// `Event, Site, Date, Round`.
    pub event_line: String,
    /// `ECO xx`, when the game has one.
    pub eco: Option<String>,
}

fn known(i18n: &I18n, value: &str) -> String {
    if value.trim().is_empty() {
        i18n.tr("info-unknown")
    } else {
        value.to_owned()
    }
}

fn with_rating(name: String, rating: Option<mychess_core::Rating>) -> String {
    match rating {
        Some(r) if r.value.get() > 0 => format!("{name} ({})", r.value),
        _ => name,
    }
}

/// Formats the header of a game; unknown values show as `?` (Scid keeps
/// `"?"`; partial dates read `2024.??.??`).
pub fn format_game_header(tags: &Tags, i18n: &I18n) -> HeaderView {
    let mut players_args = FluentArgs::new();
    players_args.set(
        "white",
        with_rating(known(i18n, &tags.white), tags.white_rating),
    );
    players_args.set(
        "black",
        with_rating(known(i18n, &tags.black), tags.black_rating),
    );
    let players = format!(
        "{}  {}",
        i18n.tr_args("info-players", &players_args),
        tags.result.as_str()
    );
    let mut round = FluentArgs::new();
    round.set("round", known(i18n, &tags.round));
    let mut event_args = FluentArgs::new();
    event_args.set("event", known(i18n, &tags.event));
    event_args.set("site", known(i18n, &tags.site));
    event_args.set("date", tags.date.to_string());
    event_args.set("round", i18n.tr_args("info-round", &round));
    let eco = tags.eco.map(|eco| {
        let mut args = FluentArgs::new();
        args.set("eco", eco.to_string());
        i18n.tr_args("info-eco", &args)
    });
    HeaderView {
        players,
        event_line: i18n.tr_args("info-event-line", &event_args),
        eco,
    }
}

/// A pane's body.
pub fn body<'a>(
    content: impl Into<Element<'a, Message>>,
    id: &'static str,
) -> Element<'a, Message> {
    container(content)
        .id(id)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::pane_body)
        .into()
}

/// Gives the board its premove layer, unless a modal layer is open: that
/// layer takes `Esc` whatever has the focus and would cancel the premove
/// (and swallow the key) instead of closing the modal (see
/// `BoardWidget::with_premove`).
fn premove_unless_modal<'a>(
    widget: crate::widgets::board::BoardWidget<'a>,
    modal_open: bool,
    setup: Option<PremoveSetup<'a>>,
) -> crate::widgets::board::BoardWidget<'a> {
    match setup {
        Some(setup) if !modal_open => widget.with_premove(setup),
        _ => widget,
    }
}

/// The board panel: the board column (player strips around the board).
#[allow(clippy::too_many_lines)] // the board and the panels that can sit under it
pub fn board_pane(shell: &Shell) -> Element<'_, Message> {
    let view = app_api::board_view(&shell.app);
    let appearance = app_api::appearance(&shell.app, shell.options);
    let prefs = app_api::engine_view_prefs(&shell.app);
    // No help during an online game (spec M8-3, D1330): no arrows and no
    // evaluation bars while the lock is on.
    let locked = shell.app.assist_locked();
    let hint_arrows = shell.ui.dialogs.tactics.arrows();
    let opening_arrows = shell.ui.dialogs.opening.arrows();
    let calvar_arrows = shell.ui.dialogs.calvar.arrows();
    let arrows: &[_] = if !hint_arrows.is_empty() {
        hint_arrows
    } else if !opening_arrows.is_empty() {
        opening_arrows
    } else if !calvar_arrows.is_empty() {
        calvar_arrows
    } else if locked {
        &[]
    } else {
        app_api::engine_arrows(&shell.app)
    };
    let play = &shell.ui.dialogs.play;
    let online = &shell.ui.dialogs.online;
    let fics = &shell.ui.dialogs.fics;
    // The clocks of a game against an engine are never hidden by the
    // preferences: the game cannot be played without them (D1482).
    let mut players = appearance.players;
    if play.strips.is_some() || online.strips.is_some() || fics.strips.is_some() {
        players.show_strips = true;
        players.show_clocks = true;
    }
    let widget = shell
        .ui
        .board_widget(&view, &appearance)
        .with_engine_arrows(arrows, prefs.arrows.click_to_play)
        .with_class_badge(shell.class_badge());
    // The opponent is to move: the board takes one premove (D1321).
    let setup = online
        .premove_rule
        .get()
        .zip(shell.app.online.my_color())
        .map(|(rule, color)| PremoveSetup {
            color,
            premove: shell.app.online.premove.get().map(|p| (p.from, p.to)),
            can_premove: rule,
        });
    // The same for a game on FICS (D2615).
    let setup = setup.or_else(|| {
        let game = shell.app.fics().game.as_ref()?;
        Some(PremoveSetup {
            color: game.my_color?,
            premove: shell.app.fics().premove.get().map(|p| (p.from, p.to)),
            can_premove: fics.premove_rule.get()?,
        })
    });
    // And against an engine (D4452).
    let setup = setup.or_else(|| {
        Some(PremoveSetup {
            color: shell.app.play_premove_context()?.1,
            premove: shell.app.play_premove().map(|p| (p.from, p.to)),
            can_premove: play.premove_rule.get()?,
        })
    });
    let widget = premove_unless_modal(widget, shell.modal_layer_open(), setup);
    let column = BoardColumn {
        board: widget,
        strips: play
            .strips
            .as_ref()
            .or(online.strips.as_ref())
            .or(fics.strips.as_ref())
            .unwrap_or_else(|| app_api::player_strips(&shell.app)),
        prefs: players,
        countdown: (play.strips.is_none() && online.strips.is_none() && fics.strips.is_none())
            || play.countdown,
        show_side_to_move: appearance.show_side_to_move,
        coords: appearance.board.coords,
        orientation: shell.ui.orientation,
        pieces: shell.ui.pieces(),
        i18n: &shell.i18n,
    };
    let bars = SideBars {
        eval: (prefs.eval_bar.show && !locked).then(|| app_api::eval_bar_view(&shell.app)),
        eval_label: prefs.eval_bar.label,
        lights: if locked {
            &[]
        } else {
            app_api::traffic_lights(&shell.app)
        },
    };
    let board = column.into_element_with(bars);
    // The panel of the game against an engine sits under the board.
    let content: Element<'_, Message> =
        match app_api::play_view(&shell.app, shell.ui.dialogs.play.now) {
            Some(view) => iced::widget::column![
                container(board).width(Length::Fill).height(Length::Fill),
                play_panel::premove_line(
                    shell.app.play_premove_enabled(),
                    shell.app.play_premove().is_some(),
                    &shell.i18n,
                    |on| Message::App(mychess_app::Message::Play(
                        mychess_app::PlayMessage::EnablePremove(on)
                    )),
                ),
                play_panel::bar_with(&view, shell.app.play_tactical().as_ref(), &shell.i18n),
            ]
            .into(),
            // The panel of the online game (spec M8-3c).
            None => match online_panel::panel_view(&shell.app, online.now, &shell.i18n) {
                Some(view) => iced::widget::column![
                    container(board).width(Length::Fill).height(Length::Fill),
                    online_panel::premove_switch(&shell.app, &shell.i18n),
                    online_panel::bar(&view, &shell.i18n),
                ]
                .into(),
                // The panel of the tactics training (spec M8-4c).
                None => match online_panel::watch_bar(&shell.app, &shell.i18n)
                    .or_else(|| crate::windows::tactics_panel::bar(&shell.app, &shell.i18n))
                    // The panel of the opening trainer (spec M8-5c).
                    .or_else(|| {
                        crate::windows::opening_trainer_panel::bar(&shell.app, &shell.i18n)
                    })
                    // The panel of the calculation exercise (spec M8-6c).
                    .or_else(|| {
                        crate::windows::calvar_panel::bar(
                            &shell.app,
                            &shell.ui.dialogs.calvar,
                            &shell.i18n,
                        )
                    }) {
                    Some(bar) => iced::widget::column![
                        container(board).width(Length::Fill).height(Length::Fill),
                        bar,
                    ]
                    .into(),
                    None => board,
                },
            },
        };
    body(content, "board-pane")
}

#[cfg(test)]
mod tests {
    use mychess_core::{Eco, Elo, GameResult, Rating, RatingType};

    use super::*;
    use crate::i18n::Language;

    #[test]
    fn default_tags_show_question_marks() {
        let header = format_game_header(&Tags::default(), &I18n::new(Language::EnUs));
        assert_eq!(header.players, "? - ?  *");
        assert_eq!(header.event_line, "?, ?, ????.??.??, Round ?");
        assert_eq!(header.eco, None);
    }

    #[test]
    fn ratings_result_and_eco_are_included() {
        let mut tags = Tags::default();
        tags.white = "Kasparov".to_owned();
        tags.black = "Karpov".to_owned();
        tags.white_rating = Some(Rating::new(Elo::new(2800).expect("elo"), RatingType::Elo));
        tags.result = GameResult::Draw;
        tags.eco = "B90".parse::<Eco>().ok();
        let header = format_game_header(&tags, &I18n::new(Language::PtBr));
        assert_eq!(header.players, "Kasparov (2800) - Karpov  1/2-1/2");
        assert_eq!(header.eco.as_deref(), Some("ECO B90"));
    }

    #[test]
    fn blank_names_read_as_unknown() {
        let mut tags = Tags::default();
        tags.white = "  ".to_owned();
        let header = format_game_header(&tags, &I18n::new(Language::EnUs));
        assert!(header.players.starts_with("? - "));
    }
}

#[cfg(test)]
mod premove_tests {
    use super::*;
    use crate::widgets::board::geometry::Orientation;
    use crate::widgets::board::marks::MarkBrush;
    use crate::widgets::board::pieces::PieceSet;
    use crate::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
    use mychess_core::{Color as Side, Position};

    #[test]
    fn a_modal_layer_takes_the_premove_layer_off_the_board() {
        let position = Position::new();
        let (brush, pieces, caches) = (
            MarkBrush::default(),
            PieceSet::default(),
            BoardCaches::default(),
        );
        let data = BoardData {
            position: &position,
            last_move: None,
            marks: &[],
            orientation: Orientation::White,
            options: BoardOptions::default(),
            accepts_moves: false,
            brush: &brush,
            pieces: &pieces,
            animation: None,
        };
        let rule = |_, _| true;
        let setup = || {
            Some(PremoveSetup {
                color: Side::White,
                premove: None,
                can_premove: &rule,
            })
        };
        let plain = || BoardWidget::new(data, &caches);
        assert!(premove_unless_modal(plain(), false, setup()).has_premove());
        assert!(!premove_unless_modal(plain(), true, setup()).has_premove());
        assert!(!premove_unless_modal(plain(), false, None).has_premove());
    }
}
