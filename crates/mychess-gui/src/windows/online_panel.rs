//! The panel under the board during an online game, and the permanent
//! notice of the lock of the assistance (spec M8-3c, D1325, D1330,
//! D1472-D1477).
//!
//! [`panel_view`] reads the application once and produces a plain
//! description ([`PanelView`]); [`bar`] draws it. The buttons change by
//! themselves with the state of the game: Abort before two plies and
//! Resign after, the answers to a draw or a takeback offered, the claim of
//! the victory over an absent opponent.

use std::fmt::Write as _;
use std::time::Instant;

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, container, row, space, text, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::AppState;
use mychess_app::online::{Link, OnlinePhase, PlayerInfo, PlayingState};
use mychess_core::Color;

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::windows::online_text;
use crate::windows::online_ui::{OnlineUi, OnlineUiMessage};

/// Id of the panel under the board.
pub const ONLINE_BAR_ID: &str = "online-bar";
/// Id of the notice of the lock of the assistance.
pub const ONLINE_LOCK_ID: &str = "online-lock-banner";

/// The opponent left (D1325).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoneView {
    /// Seconds until the victory can be claimed.
    pub wait_secs: u64,
    /// The claim is open.
    pub claimable: bool,
}

/// What the panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    /// The Fluent key of the status line (ignored when `result` is set).
    pub status_key: &'static str,
    /// How the game ended, for the user, once it did.
    pub result: Option<String>,
    /// The opponent, with title and rating.
    pub opponent: String,
    /// The game is on (live or reconnecting).
    pub live: bool,
    /// The game can still be aborted.
    pub can_abort: bool,
    /// The game can be resigned.
    pub can_resign: bool,
    /// The opponent offers a draw.
    pub draw_offered: bool,
    /// The user's own offer is waiting.
    pub own_draw_pending: bool,
    /// The opponent proposes a takeback.
    pub takeback_proposed: bool,
    /// A takeback can be proposed now (D3011).
    pub can_propose_takeback: bool,
    /// The finished game can be played again against the same opponent (D3012).
    pub can_rematch: bool,
    /// The opponent left.
    pub gone: Option<GoneView>,
    /// The state of the event stream.
    pub link: Link,
    /// The latency of the last move, in milliseconds.
    pub ping_ms: Option<u128>,
    /// A premove waits for the turn of the user.
    pub premove: bool,
    /// The game of the editor that was there before can be put back.
    pub can_restore: bool,
    /// Rated or casual, and the clock.
    pub info: String,
}

fn describe(player: &PlayerInfo) -> String {
    let mut text = String::new();
    if let Some(title) = &player.title {
        text.push_str(title);
        text.push(' ');
    }
    text.push_str(&player.name);
    if let Some(rating) = player.rating {
        let mark = if player.provisional { "?" } else { "" };
        let _ = write!(text, " ({rating}{mark})");
    }
    text
}

/// The clock of the game as `10+5` (minutes and seconds).
fn clock_label(clock: Option<(u64, u64)>) -> String {
    clock.map_or_else(String::new, |(initial, increment)| {
        format!("{}+{}", initial / 60_000, increment / 1000)
    })
}

/// What the panel shows at `now`, or `None` when no online game is on
/// screen.
pub fn panel_view(app: &AppState, now: Instant, i18n: &I18n) -> Option<PanelView> {
    let online = &app.online;
    if !matches!(online.phase, OnlinePhase::Playing(_)) {
        return None;
    }
    let game = online.game.as_ref()?;
    let over = game.is_over();
    let live = online.phase.is_live();
    let opponent = match game.my_color {
        Color::White => &game.black,
        Color::Black => &game.white,
    };
    let status_key = if online.phase == OnlinePhase::Playing(PlayingState::Reconnecting) {
        "online-status-reconnecting"
    } else if over {
        "online-status-over"
    } else if game.my_turn() {
        "online-status-your-turn"
    } else {
        "online-status-opponent-turn"
    };
    let own_draw_pending = match game.my_color {
        Color::White => game.wdraw,
        Color::Black => game.bdraw,
    };
    let can_abort = online.can_abort();
    let gone = online.opponent_gone.filter(|_| live).map(|g| GoneView {
        wait_secs: OnlineUi::claim_wait(app, now).map_or(0, |d| d.as_secs()),
        claimable: g.claimable(now),
    });
    let kind = if game.rated {
        "online-info-rated"
    } else {
        "online-info-casual"
    };
    let mut args = FluentArgs::new();
    args.set("time", clock_label(game.clock));
    Some(PanelView {
        status_key,
        result: online
            .finished
            .as_ref()
            .map(|outcome| online_text::outcome_text(i18n, *outcome, game.my_color)),
        opponent: describe(opponent),
        live,
        can_abort,
        can_resign: live && !over && !can_abort,
        draw_offered: online.draw_offered_to_me(),
        own_draw_pending,
        takeback_proposed: online.takeback_proposed_to_me(),
        can_propose_takeback: online.can_propose_takeback(),
        can_rematch: online.can_rematch(),
        gone,
        link: online.link,
        ping_ms: app
            .prefs
            .online
            .show_ping
            .then_some(online.ping)
            .flatten()
            .map(|d| d.as_millis()),
        premove: online.premove.is_set(),
        can_restore: app.online_has_parked_game(),
        info: i18n.tr_args(kind, &args),
    })
}

/// The visible premove switch of the game on Lichess (D4452): the
/// preference `[online] premove`, changed in place. Only while the user can
/// still move in a live game (the note that a premove waits is on the bar).
pub fn premove_switch<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let base = app.prefs.online.as_ref().clone();
    crate::windows::play_panel::premove_line(base.premove, false, i18n, move |on| {
        let mut next = base.clone();
        next.premove = on;
        Message::App(mychess_app::Message::Prefs(mychess_app::PrefsMessage::Set(
            mychess_app::PrefChange::Online(Box::new(next)),
        )))
    })
}

fn send(message: OnlineUiMessage) -> Message {
    Message::Online(message)
}

fn action<'a>(
    label: String,
    tip: String,
    message: Option<OnlineUiMessage>,
    primary: bool,
) -> Element<'a, Message> {
    let b = button(text(label).size(13))
        .padding([4, 10])
        .on_press_maybe(message.map(send));
    let b = if primary {
        b.style(button::primary)
    } else {
        b.style(button::secondary)
    };
    tooltip(b, text(tip).size(12), tooltip::Position::Top)
        .style(theme::tooltip_box)
        .into()
}

fn link_key(link: Link) -> &'static str {
    match link {
        Link::Up => "online-link-up",
        Link::Reconnecting => "online-link-reconnecting",
        _ => "online-link-down",
    }
}

/// The panel under the board.
#[allow(clippy::too_many_lines)] // one flat list of buttons per state
pub fn bar<'a>(view: &PanelView, i18n: &I18n) -> Element<'a, Message> {
    let mut buttons = row![].spacing(6).align_y(Alignment::Center);
    if view.live {
        if view.draw_offered {
            buttons = buttons
                .push(action(
                    i18n.tr("online-draw-accept"),
                    i18n.tr("online-draw-accept-tip"),
                    Some(OnlineUiMessage::AnswerDraw(true)),
                    true,
                ))
                .push(action(
                    i18n.tr("online-draw-decline"),
                    i18n.tr("online-draw-decline-tip"),
                    Some(OnlineUiMessage::AnswerDraw(false)),
                    false,
                ));
        } else {
            buttons = buttons.push(action(
                i18n.tr("online-offer-draw"),
                i18n.tr("online-offer-draw-tip"),
                (!view.own_draw_pending).then_some(OnlineUiMessage::OfferDraw),
                false,
            ));
        }
        if view.takeback_proposed {
            buttons = buttons
                .push(action(
                    i18n.tr("online-takeback-accept"),
                    i18n.tr("online-takeback-accept-tip"),
                    Some(OnlineUiMessage::AnswerTakeback(true)),
                    true,
                ))
                .push(action(
                    i18n.tr("online-takeback-decline"),
                    i18n.tr("online-takeback-decline-tip"),
                    Some(OnlineUiMessage::AnswerTakeback(false)),
                    false,
                ));
        }
        if view.can_propose_takeback {
            buttons = buttons.push(action(
                i18n.tr("online-takeback-propose"),
                i18n.tr("online-takeback-propose-tip"),
                Some(OnlineUiMessage::ProposeTakeback),
                false,
            ));
        }
        if let Some(gone) = view.gone {
            buttons = buttons
                .push(action(
                    i18n.tr("online-claim"),
                    i18n.tr("online-claim-tip"),
                    gone.claimable.then_some(OnlineUiMessage::Claim),
                    true,
                ))
                .push(action(
                    i18n.tr("online-claim-draw"),
                    i18n.tr("online-claim-draw-tip"),
                    gone.claimable.then_some(OnlineUiMessage::ClaimDraw),
                    false,
                ));
        }
        buttons = if view.can_abort {
            buttons.push(action(
                i18n.tr("online-abort"),
                i18n.tr("online-abort-tip"),
                Some(OnlineUiMessage::Abort),
                false,
            ))
        } else {
            buttons.push(action(
                i18n.tr("online-resign"),
                i18n.tr("online-resign-tip"),
                view.can_resign.then_some(OnlineUiMessage::Resign),
                false,
            ))
        };
    } else {
        if view.can_restore {
            buttons = buttons.push(action(
                i18n.tr("online-restore"),
                i18n.tr("online-restore-tip"),
                Some(OnlineUiMessage::Restore),
                false,
            ));
        }
        if view.can_rematch {
            buttons = buttons.push(action(
                i18n.tr("online-rematch"),
                i18n.tr("online-rematch-tip"),
                Some(OnlineUiMessage::Rematch),
                true,
            ));
        }
        buttons = buttons.push(action(
            i18n.tr("online-close"),
            i18n.tr("online-close-tip"),
            Some(OnlineUiMessage::Dismiss),
            true,
        ));
    }
    let headline = view
        .result
        .clone()
        .unwrap_or_else(|| i18n.tr(view.status_key));
    let mut versus = FluentArgs::new();
    versus.set("name", view.opponent.as_str());
    let mut info = column![
        text(headline).size(14),
        text(format!(
            "{}  ·  {}",
            i18n.tr_args("online-versus", &versus),
            view.info
        ))
        .size(12)
        .style(theme::muted_text),
    ]
    .spacing(2);
    if let Some(gone) = view.gone {
        let mut args = FluentArgs::new();
        args.set("secs", i64::try_from(gone.wait_secs).unwrap_or(i64::MAX));
        let key = if gone.claimable {
            "online-gone-claimable"
        } else {
            "online-gone-waiting"
        };
        info = info.push(text(i18n.tr_args(key, &args)).size(12).style(text::danger));
    }
    if view.premove {
        info = info.push(
            text(i18n.tr("online-premove-set"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    let mut status = i18n.tr(link_key(view.link));
    if let Some(ms) = view.ping_ms {
        let mut args = FluentArgs::new();
        args.set("ms", i64::try_from(ms).unwrap_or(i64::MAX));
        status = format!("{status}  ·  {}", i18n.tr_args("online-ping", &args));
    }
    container(
        row![
            info,
            space::horizontal(),
            text(status).size(12).style(theme::muted_text),
            buttons,
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .id(ONLINE_BAR_ID)
    .padding([6, 12])
    .width(Length::Fill)
    .style(theme::status_bar)
    .into()
}

/// The bar under the board while a game of other players is watched
/// (D3004): who plays, the delay warning and the way out.
pub fn watch_bar<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let watch = app.online.watch.as_ref()?;
    let game = watch.game.as_ref()?;
    let who = |p: &mychess_app::online::WatchPlayerView| {
        let mut line = String::new();
        if let Some(title) = &p.title {
            line.push_str(title);
            line.push(' ');
        }
        line.push_str(&p.name);
        if let Some(rating) = p.rating {
            let _ = write!(line, " ({rating})");
        }
        line
    };
    let mut names = FluentArgs::new();
    names.set("white", who(&game.white));
    names.set("black", who(&game.black));
    let status_key = if game.is_over() {
        "online-watch-over"
    } else if game.complete {
        "online-watch-live"
    } else {
        "online-watch-incomplete"
    };
    let info = column![
        text(i18n.tr_args("online-watch-names", &names)).size(14),
        text(i18n.tr(status_key)).size(12).style(theme::muted_text),
    ]
    .spacing(2);
    Some(
        container(
            row![
                info,
                space::horizontal(),
                action(
                    i18n.tr("online-watch-stop"),
                    i18n.tr("online-watch-stop-tip"),
                    Some(OnlineUiMessage::StopWatch),
                    true,
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .width(Length::Fill)
        .style(theme::status_bar)
        .into(),
    )
}

/// The permanent notice that the assistance is off, with the way out when
/// the connection is gone (D1332). `None` when nothing is locked.
pub fn lock_banner<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    if !app.assist_locked() {
        return None;
    }
    let mut line = row![
        text(i18n.tr("online-assist-off")).size(12),
        space::horizontal(),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if app.can_remove_lock() {
        line = line.push(
            button(text(i18n.tr("online-remove-lock")).size(12))
                .padding([2, 8])
                .style(button::secondary)
                .on_press(send(OnlineUiMessage::AskRemoveLock)),
        );
    }
    Some(
        container(line)
            .id(ONLINE_LOCK_ID)
            .padding([3, 12])
            .width(Length::Fill)
            .style(theme::status_bar)
            .into(),
    )
}
