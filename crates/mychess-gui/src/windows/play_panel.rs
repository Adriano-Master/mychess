//! The panel under the board during a game against an engine and the boxes
//! of the game: the question of the book or the line, the end of the game
//! and the engine that was lost (spec M8-1b, D1221, D1239, D1254-D1262).
//!
//! Everything is drawn from the [`PlayView`] the application offers; the
//! buttons send [`PlayUiMessage`]s.

use iced::widget::{button, column, container, row, space, text, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::engines::{EngineEntry, Protocol};
use mychess_app::tactical_game::TacticalView;
use mychess_app::{
    AppState, CoachGrade, CoachView, PlayEnd, PlayNotice, PlayPhase, PlayQuestion, PlayView,
};
use mychess_core::{Color, GameResult};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::play_dialog::{PlayConfirm, PlayUiMessage};

/// Id of the panel under the board.
pub const PLAY_BAR_ID: &str = "play-bar";
/// Id of the box that confirms Resign, Restart or Quit.
pub const PLAY_CONFIRM_BOX_ID: &str = "play-confirm-box";
/// Id of the box with the question of the book or the line.
pub const PLAY_ASK_BOX_ID: &str = "play-ask-box";
/// Id of the box of the end of the game.
pub const PLAY_END_BOX_ID: &str = "play-end-box";
/// Id of the box shown when the engine was lost.
pub const PLAY_LOST_BOX_ID: &str = "play-lost-box";

/// The box a game asks for, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayBox {
    /// A question to the player.
    Ask,
    /// The game is over.
    End,
    /// The engine is gone.
    Lost,
}

/// Which box the state of the game calls for.
pub fn box_for(view: &PlayView) -> Option<PlayBox> {
    if view.question.is_some() {
        Some(PlayBox::Ask)
    } else if matches!(view.phase, PlayPhase::Ended(_)) {
        Some(PlayBox::End)
    } else if view.engine_lost {
        Some(PlayBox::Lost)
    } else {
        None
    }
}

fn send(message: PlayUiMessage) -> Message {
    Message::PlayUi(message)
}

/// The Fluent key of the line that says whose turn it is.
pub fn status_key(view: &PlayView) -> &'static str {
    match view.phase {
        PlayPhase::Player => "play-status-your-turn",
        PlayPhase::Opponent => "play-status-engine-thinking",
        PlayPhase::Asking(_) => "play-status-question",
        PlayPhase::Paused { lost: true } => "play-status-engine-lost",
        PlayPhase::Paused { .. } => "play-status-paused",
        PlayPhase::Ended(_) => "play-status-over",
        _ => "play-status-closed",
    }
}

fn tipped<'a>(
    label: String,
    tip: String,
    message: Option<Message>,
    primary: bool,
) -> Element<'a, Message> {
    let b = button(text(label).size(13))
        .padding([4, 10])
        .on_press_maybe(message);
    let b = if primary {
        b.style(button::primary)
    } else {
        b.style(button::secondary)
    };
    tooltip(b, text(tip).size(12), tooltip::Position::Top)
        .style(theme::tooltip_box)
        .into()
}

/// The panel under the board: the state of the game and its buttons.
pub fn bar<'a>(view: &PlayView, i18n: &I18n) -> Element<'a, Message> {
    bar_with(view, None, i18n)
}

/// The panel under the board, with the line of the tactical game when the
/// game is one (M8-7, D2471).
pub fn bar_with<'a>(
    view: &PlayView,
    tactical: Option<&TacticalView>,
    i18n: &I18n,
) -> Element<'a, Message> {
    // Resign is possible while the game is on, also when it is paused.
    let live = crate::windows::play_dialog::in_progress(Some(view.phase));
    let mut buttons = row![].spacing(6).align_y(Alignment::Center);
    if view.can_resume {
        buttons = buttons.push(tipped(
            i18n.tr("play-resume"),
            i18n.tr("play-resume-tip"),
            Some(send(PlayUiMessage::Resume)),
            true,
        ));
    }
    buttons = buttons
        .push(tipped(
            i18n.tr("play-take-back"),
            i18n.tr("play-take-back-tip"),
            view.can_take_back.then(|| send(PlayUiMessage::TakeBack)),
            false,
        ))
        .push(tipped(
            i18n.tr("play-offer-draw"),
            i18n.tr("play-offer-draw-tip"),
            view.can_offer_draw.then(|| send(PlayUiMessage::OfferDraw)),
            false,
        ))
        .push(tipped(
            i18n.tr("play-resign"),
            i18n.tr("play-resign-tip"),
            live.then(|| send(PlayUiMessage::Resign)),
            false,
        ))
        .push(tipped(
            i18n.tr("play-restart"),
            i18n.tr("play-restart-tip"),
            Some(send(PlayUiMessage::Restart)),
            false,
        ))
        .push(tipped(
            i18n.tr("play-quit"),
            i18n.tr("play-quit-tip"),
            Some(send(PlayUiMessage::Quit)),
            false,
        ));
    let mut info = column![text(i18n.tr(status_key(view))).size(14)].spacing(2);
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("name", view.opponent.name.clone());
    info = info.push(
        text(i18n.tr_args("play-versus", &args))
            .size(12)
            .style(theme::muted_text),
    );
    if view.pondering {
        info = info.push(
            text(i18n.tr("play-pondering"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    if let Some(tactical) = tactical {
        info = info.push(
            text(crate::windows::tactical_dialog::line_text(tactical, i18n))
                .size(12)
                .style(theme::muted_text),
        );
    }
    if let Some(coach) = &view.coach {
        info = info.push(
            text(coach_line(coach, i18n))
                .size(12)
                .style(coach_style(coach)),
        );
    }
    container(
        row![info, space::horizontal(), buttons]
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .id(PLAY_BAR_ID)
    .padding([6, 12])
    .width(Length::Fill)
    .style(theme::status_bar)
    .into()
}

/// Id of the line with the premove switch under the board.
pub const PREMOVE_BAR_ID: &str = "premove-bar";

/// The line under the board with the premove switch (D4452): a box that is
/// always visible while a game is on, so the premove is never on without
/// a clear sign. `set` adds the note that a premove is waiting.
pub fn premove_line<'a>(
    enabled: bool,
    set: bool,
    i18n: &I18n,
    on_toggle: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    let toggle = tooltip(
        crate::windows::annotate_dialog::check(i18n.tr("play-premove"), enabled, true, on_toggle),
        text(i18n.tr("play-premove-tip")).size(12),
        tooltip::Position::Top,
    )
    .style(theme::tooltip_box);
    let mut line = row![toggle].spacing(12).align_y(Alignment::Center);
    if set {
        line = line.push(
            text(i18n.tr("play-premove-set"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    container(line)
        .id(PREMOVE_BAR_ID)
        .padding([2, 12])
        .width(Length::Fill)
        .style(theme::status_bar)
        .into()
}

/// The tone of the line of the coach: a warning for a weak move.
fn coach_style(coach: &CoachView) -> fn(&iced::Theme) -> text::Style {
    match coach.last.as_ref().map(|v| v.grade) {
        Some(CoachGrade::Mistake | CoachGrade::Blunder) if !coach.lost => text::danger,
        _ => theme::muted_text,
    }
}

/// The short remark of the coach for the panel (D2425): the word of the
/// grade, the loss in pawns when asked for, and the better move when the
/// application passed it.
pub fn coach_line(coach: &CoachView, i18n: &I18n) -> String {
    let remark = if coach.lost {
        i18n.tr("play-coach-lost")
    } else if let Some(v) = &coach.last
        && !coach.busy
    {
        let word = i18n.tr(v.grade.message_key());
        let mut text = if coach.numbers && v.grade.is_weak() && !v.decisive {
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("word", word);
            args.set("loss", pawns(v.loss_cp, i18n));
            i18n.tr_args("play-coach-loss", &args)
        } else {
            word
        };
        if let Some(san) = &v.best_san {
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("text", text);
            args.set("san", san.clone());
            text = i18n.tr_args("play-coach-better", &args);
        }
        text
    } else if coach.busy {
        i18n.tr("play-coach-busy")
    } else {
        i18n.tr("play-coach-wait")
    };
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("remark", remark);
    i18n.tr_args("play-coach-line", &args)
}

/// `85` centipawns as "0.9" ("0,9" in Portuguese); whole tenths, rounded
/// half up, so the text does not depend on float rounding.
fn pawns(loss_cp: i32, i18n: &I18n) -> String {
    let tenths = loss_cp.max(0).saturating_add(5) / 10;
    let sep = if i18n.language() == crate::i18n::Language::PtBr {
        ','
    } else {
        '.'
    };
    format!("{}{sep}{}", tenths / 10, tenths % 10)
}

/// The confirmation of Resign, Restart or Quit in the middle of a game
/// (D1480): Yes carries it out, No (and Esc) goes back to the game.
pub fn confirm_box<'a>(kind: PlayConfirm, i18n: &I18n) -> Element<'a, Message> {
    let (title, body) = match kind {
        PlayConfirm::Resign => ("play-confirm-resign-title", "play-confirm-resign"),
        PlayConfirm::Restart => ("play-confirm-restart-title", "play-confirm-restart"),
        PlayConfirm::Quit => ("play-confirm-quit-title", "play-confirm-quit"),
    };
    modal(Card {
        id: PLAY_CONFIRM_BOX_ID,
        title: i18n.tr(title),
        body: text(i18n.tr(body)).into(),
        buttons: vec![
            button(text(i18n.tr("play-no")))
                .style(button::secondary)
                .on_press(send(PlayUiMessage::Confirm(false)))
                .into(),
            button(text(i18n.tr("play-yes")))
                .style(button::primary)
                .on_press(send(PlayUiMessage::Confirm(true)))
                .into(),
        ],
        width: 440.0,
    })
}

/// The question of the book or the line (D1234, D1235): Yes goes on out of
/// it, No takes the move back.
pub fn ask_box<'a>(question: PlayQuestion, i18n: &I18n) -> Element<'a, Message> {
    let (title, body) = match question {
        PlayQuestion::LeaveBook { .. } => ("play-ask-book-title", "play-ask-book"),
        _ => ("play-ask-line-title", "play-ask-line"),
    };
    modal(Card {
        id: PLAY_ASK_BOX_ID,
        title: i18n.tr(title),
        body: text(i18n.tr(body)).into(),
        buttons: vec![
            button(text(i18n.tr("play-no")))
                .style(button::secondary)
                .on_press(send(PlayUiMessage::Answer(false)))
                .into(),
            button(text(i18n.tr("play-yes")))
                .style(button::primary)
                .on_press(send(PlayUiMessage::Answer(true)))
                .into(),
        ],
        width: 440.0,
    })
}

/// The Fluent key of what the result means for the player.
pub fn outcome_key(result: GameResult, player: Color) -> &'static str {
    match (result, player) {
        (GameResult::White, Color::White) | (GameResult::Black, Color::Black) => "play-end-won",
        (GameResult::White, Color::Black) | (GameResult::Black, Color::White) => "play-end-lost",
        (GameResult::Draw, _) => "play-end-draw",
        _ => "play-end-no-result",
    }
}

/// Whether the database of the current slot refuses new games (D1231).
pub fn current_base_read_only(app: &AppState) -> bool {
    app.bases
        .slot(app.bases.current())
        .is_some_and(|slot| slot.read_only)
}

/// The end of the game: the result and the reason, with Save game, Restart
/// and Close.
pub fn end_box<'a>(
    end: PlayEnd,
    view: &PlayView,
    read_only: bool,
    i18n: &I18n,
) -> Element<'a, Message> {
    end_box_with(end, view, None, read_only, i18n)
}

/// The end of the game, with the score and the change of Elo of a tactical
/// game (M8-7, D2462).
pub fn end_box_with<'a>(
    end: PlayEnd,
    view: &PlayView,
    tactical: Option<&TacticalView>,
    read_only: bool,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut body = column![
        text(i18n.tr(outcome_key(view.result, view.player_color))).size(16),
        text(i18n.tr(PlayNotice::Ended(end).message_key())).size(13),
        text(format!(
            "{} {}",
            i18n.tr("play-end-result"),
            view.result.as_str()
        ))
        .size(13)
        .style(theme::muted_text),
    ]
    .spacing(6);
    if let Some(tactical) = tactical {
        body = body.push(
            text(crate::windows::tactical_dialog::stats_text(
                &tactical.stats,
                i18n,
            ))
            .size(12)
            .style(theme::muted_text),
        );
        if let Some(line) = crate::windows::tactical_dialog::next_elo_text(tactical, i18n) {
            body = body.push(text(line).size(12).style(theme::muted_text));
        }
    }
    if read_only {
        body = body.push(
            text(i18n.tr("play-end-read-only"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    modal(Card {
        id: PLAY_END_BOX_ID,
        title: i18n.tr("play-end-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("play-end-save")))
                .style(button::secondary)
                .on_press(send(PlayUiMessage::SaveGame))
                .into(),
            button(text(i18n.tr("play-restart")))
                .style(button::secondary)
                .on_press(send(PlayUiMessage::Restart))
                .into(),
            button(text(i18n.tr("game-cmd-rematch")))
                .style(button::secondary)
                .on_press(send(PlayUiMessage::Rematch))
                .into(),
            button(text(i18n.tr("play-end-close")))
                .style(button::primary)
                .on_press(send(PlayUiMessage::Quit))
                .into(),
        ],
        width: 460.0,
    })
}

/// The engine is gone (D1239): another UCI engine takes over, or the game
/// ends without a result.
pub fn lost_box<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let others: Vec<&EngineEntry> = app
        .engines
        .engines()
        .iter()
        .filter(|e| e.protocol == Protocol::Uci)
        .collect();
    let mut body = column![text(i18n.tr("play-lost-message")).size(13)].spacing(8);
    if others.is_empty() {
        body = body.push(
            text(i18n.tr("play-no-engine"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    for entry in others {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("name", entry.name.clone());
        body = body.push(
            button(text(i18n.tr_args("play-lost-switch", &args)).size(13))
                .style(button::secondary)
                .on_press(send(PlayUiMessage::SwitchEngine(entry.id))),
        );
    }
    modal(Card {
        id: PLAY_LOST_BOX_ID,
        title: i18n.tr("play-lost-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("play-lost-end")))
                .style(button::danger)
                .on_press(send(PlayUiMessage::Quit))
                .into(),
        ],
        width: 460.0,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::OpponentInfo;

    use super::*;
    use crate::i18n::Language;
    use crate::shell::Shell;

    fn view_in(phase: PlayPhase) -> PlayView {
        PlayView {
            phase,
            player_color: Color::White,
            opponent: OpponentInfo {
                name: "Fake".to_owned(),
                rating: None,
            },
            white_time: Duration::from_secs(60),
            black_time: Duration::from_secs(60),
            countdown: true,
            question: match phase {
                PlayPhase::Asking(q) => Some(q),
                _ => None,
            },
            result: GameResult::Unknown,
            can_resume: matches!(phase, PlayPhase::Paused { lost: false }),
            engine_lost: matches!(phase, PlayPhase::Paused { lost: true }),
            can_take_back: false,
            can_offer_draw: false,
            pondering: false,
            coach: None,
        }
    }

    fn sim(element: Element<'_, Message>) -> Simulator<'_, Message> {
        Simulator::with_size(Settings::default(), Size::new(900.0, 400.0), element)
    }

    fn clicked(element: Element<'_, Message>, label: &str) -> Vec<Message> {
        let mut s = sim(element);
        s.click(label).expect("button");
        s.into_messages().collect()
    }

    #[test]
    fn the_box_follows_the_phase() {
        assert_eq!(box_for(&view_in(PlayPhase::Player)), None);
        assert_eq!(box_for(&view_in(PlayPhase::Opponent)), None);
        assert_eq!(box_for(&view_in(PlayPhase::Paused { lost: false })), None);
        assert_eq!(
            box_for(&view_in(PlayPhase::Paused { lost: true })),
            Some(PlayBox::Lost)
        );
        assert_eq!(
            box_for(&view_in(PlayPhase::Ended(PlayEnd::Stalemate))),
            Some(PlayBox::End)
        );
    }

    #[test]
    fn the_bar_has_resign_restart_and_quit_and_resume_only_when_paused() {
        let i18n = I18n::new(Language::EnUs);
        let running = view_in(PlayPhase::Player);
        let mut s = sim(bar(&running, &i18n));
        for label in ["Resign", "Restart", "Quit", "Your turn"] {
            assert!(s.find(label).is_ok(), "{label}");
        }
        assert!(s.find("Resume").is_err());
        let paused = view_in(PlayPhase::Paused { lost: false });
        let mut s = sim(bar(&paused, &i18n));
        assert!(s.find("Resume").is_ok());
        let sent = clicked(bar(&paused, &i18n), "Resume");
        assert!(matches!(
            sent.as_slice(),
            [Message::PlayUi(PlayUiMessage::Resume)]
        ));
        let sent = clicked(bar(&running, &i18n), "Resign");
        assert!(matches!(
            sent.as_slice(),
            [Message::PlayUi(PlayUiMessage::Resign)]
        ));
    }

    #[test]
    fn take_back_and_offer_draw_follow_the_view_and_send_their_messages() {
        let i18n = I18n::new(Language::EnUs);
        let mut view = view_in(PlayPhase::Player);
        view.can_take_back = false;
        view.can_offer_draw = false;
        assert!(clicked(bar(&view, &i18n), "Take back").is_empty());
        assert!(clicked(bar(&view, &i18n), "Offer draw").is_empty());
        view.can_take_back = true;
        view.can_offer_draw = true;
        let sent = clicked(bar(&view, &i18n), "Take back");
        assert!(matches!(
            sent.as_slice(),
            [Message::PlayUi(PlayUiMessage::TakeBack)]
        ));
        let sent = clicked(bar(&view, &i18n), "Offer draw");
        assert!(matches!(
            sent.as_slice(),
            [Message::PlayUi(PlayUiMessage::OfferDraw)]
        ));
        let pt = I18n::new(Language::PtBr);
        let mut s = sim(bar(&view, &pt));
        for label in ["Desfazer lance", "Oferecer empate"] {
            assert!(s.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn the_end_box_names_the_draw_by_agreement() {
        let i18n = I18n::new(Language::EnUs);
        let mut view = view_in(PlayPhase::Ended(PlayEnd::DrawAgreed));
        view.result = GameResult::Draw;
        let mut s = sim(end_box(PlayEnd::DrawAgreed, &view, false, &i18n));
        assert!(s.find("You drew.").is_ok());
        assert!(s.find("Draw by agreement.").is_ok());
    }

    #[test]
    fn the_bar_is_translated() {
        let pt = I18n::new(Language::PtBr);
        let mut s = sim(bar(&view_in(PlayPhase::Opponent), &pt));
        for label in ["Desistir", "Reiniciar", "Sair", "A engine está pensando..."] {
            assert!(s.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn resign_is_off_once_the_game_is_over() {
        let i18n = I18n::new(Language::EnUs);
        let over = view_in(PlayPhase::Ended(PlayEnd::Stalemate));
        let sent = clicked(bar(&over, &i18n), "Resign");
        assert!(sent.is_empty(), "{sent:?}");
    }

    #[test]
    fn the_question_answers_yes_or_no() {
        let i18n = I18n::new(Language::EnUs);
        let q = PlayQuestion::LeaveBook {
            mv: mychess_core::Move::Null,
        };
        let yes = clicked(ask_box(q, &i18n), "Yes");
        assert!(matches!(
            yes.as_slice(),
            [Message::PlayUi(PlayUiMessage::Answer(true))]
        ));
        let no = clicked(ask_box(q, &i18n), "No");
        assert!(matches!(
            no.as_slice(),
            [Message::PlayUi(PlayUiMessage::Answer(false))]
        ));
        let line = PlayQuestion::LeaveLine {
            mv: mychess_core::Move::Null,
        };
        let mut s = sim(ask_box(line, &I18n::new(Language::PtBr)));
        assert!(s.find("Sair da linha?").is_ok());
    }

    #[test]
    fn the_end_box_says_the_result_and_offers_save_restart_close() {
        let i18n = I18n::new(Language::EnUs);
        let mut view = view_in(PlayPhase::Ended(PlayEnd::Checkmate {
            winner: Color::Black,
        }));
        view.result = GameResult::Black;
        let end = PlayEnd::Checkmate {
            winner: Color::Black,
        };
        let mut s = sim(end_box(end, &view, false, &i18n));
        assert!(s.find("You lost.").is_ok());
        assert!(s.find("Result: 0-1").is_ok());
        assert!(s.find("This database is read-only.").is_err());
        for (label, expected) in [("Save game", 0), ("Restart", 1), ("Close", 2)] {
            let sent = clicked(end_box(end, &view, false, &i18n), label);
            let ok = matches!(
                (sent.as_slice(), expected),
                ([Message::PlayUi(PlayUiMessage::SaveGame)], 0)
                    | ([Message::PlayUi(PlayUiMessage::Restart)], 1)
                    | ([Message::PlayUi(PlayUiMessage::Quit)], 2)
            );
            assert!(ok, "{label}: {sent:?}");
        }
    }

    #[test]
    fn the_end_box_warns_about_a_read_only_database() {
        let i18n = I18n::new(Language::EnUs);
        let mut view = view_in(PlayPhase::Ended(PlayEnd::Stalemate));
        view.result = GameResult::Draw;
        let mut s = sim(end_box(PlayEnd::Stalemate, &view, true, &i18n));
        assert!(s.find("You drew.").is_ok());
        assert!(
            s.find("The current database is read-only: the game is saved to a PGN file.")
                .is_ok()
        );
    }

    #[test]
    fn outcomes_are_seen_from_the_player() {
        assert_eq!(outcome_key(GameResult::White, Color::White), "play-end-won");
        assert_eq!(
            outcome_key(GameResult::White, Color::Black),
            "play-end-lost"
        );
        assert_eq!(outcome_key(GameResult::Draw, Color::Black), "play-end-draw");
        assert_eq!(
            outcome_key(GameResult::Unknown, Color::White),
            "play-end-no-result"
        );
    }

    #[test]
    fn the_lost_box_ends_the_game_and_the_read_only_check_reads_the_slot() {
        let shell = Shell::new(None);
        let i18n = I18n::new(Language::EnUs);
        let sent = clicked(lost_box(&shell.app, &i18n), "End game");
        assert!(matches!(
            sent.as_slice(),
            [Message::PlayUi(PlayUiMessage::Quit)]
        ));
        assert!(!current_base_read_only(&shell.app));
    }
}
