//! The windows of the online play: connect, find a game, challenge a user,
//! the challenges received, and the confirmations (spec M8-3c, D1471-D1479).
//!
//! The windows are drawn from the state of the application (`app.online`)
//! and the forms of [`OnlineUi`]; the buttons send [`OnlineUiMessage`]s.

use fluent_bundle::FluentArgs;
use iced::widget::{button, checkbox, column, container, radio, row, text, text_input};
use iced::{Alignment, Element, Length};
use mychess_app::AppState;
use mychess_app::online::{OnlineColor, OnlinePhase, StoreChoice};
use mychess_core::Role;

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::online_text;
use crate::windows::online_ui::{
    ONLINE_MINUTES_ID, ONLINE_TOKEN_ID, ONLINE_USER_ID, ONLINE_WATCH_ID, OnlineConfirm, OnlineUi,
    OnlineUiMessage, PRESETS, Secret, TimeForm,
};

/// Id of the connection window.
pub const ONLINE_CONNECT_BOX_ID: &str = "online-connect-box";
/// Id of the seek window.
pub const ONLINE_SEEK_BOX_ID: &str = "online-seek-box";
/// Id of the challenge window.
pub const ONLINE_CHALLENGE_BOX_ID: &str = "online-challenge-box";
/// Id of the list of challenges received.
pub const ONLINE_INCOMING_BOX_ID: &str = "online-incoming-box";
/// Id of the window of the game to watch.
pub const ONLINE_WATCH_BOX_ID: &str = "online-watch-box";
/// Id of the choice of the piece of a premove that promotes.
pub const ONLINE_PROMOTION_BOX_ID: &str = "online-promotion-box";
/// Id of the confirmation.
pub const ONLINE_CONFIRM_BOX_ID: &str = "online-confirm-box";

fn send(message: OnlineUiMessage) -> Message {
    Message::Online(message)
}

fn muted<'a>(content: String) -> Element<'a, Message> {
    text(content).size(12).style(theme::muted_text).into()
}

/// The last failure of the online play, when there is one, in red.
fn failure<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let notice = app.online.notice.as_ref()?;
    online_text::is_failure(notice).then(|| {
        text(online_text::notice_text(i18n, notice))
            .size(13)
            .style(text::danger)
            .into()
    })
}

fn cancel_button<'a>(i18n: &I18n) -> Element<'a, Message> {
    button(text(i18n.tr("dialog-cancel")))
        .style(button::secondary)
        .on_press(send(OnlineUiMessage::Close))
        .into()
}

/// "Connect to Lichess": the steps, the token and where to keep it.
pub fn connect<'a>(app: &AppState, ui: &'a OnlineUi, i18n: &I18n) -> Element<'a, Message> {
    if let Some(wait) = app.online.oauth.as_ref() {
        return oauth_waiting(wait.url.is_some(), wait.opened, app, i18n);
    }
    let connecting = app.online.phase == OnlinePhase::Connecting;
    let token = text_input(
        &i18n.tr("online-connect-token-placeholder"),
        ui.token.expose(),
    )
    .id(ONLINE_TOKEN_ID)
    .secure(true)
    .on_input_maybe(
        (!connecting).then_some(|t: String| send(OnlineUiMessage::Token(Secret::new(t)))),
    )
    .on_submit(send(OnlineUiMessage::Submit))
    .padding(5);
    let store = |key: &str, choice: StoreChoice| -> Element<'a, Message> {
        radio(i18n.tr(key), choice, Some(ui.store), |c| {
            send(OnlineUiMessage::Store(c))
        })
        .size(16)
        .text_size(13)
        .into()
    };
    let mut body = column![
        button(text(i18n.tr("online-oauth-button")).size(13))
            .style(button::primary)
            .on_press_maybe((!connecting).then_some(send(OnlineUiMessage::OAuthLogin))),
        muted(i18n.tr("online-oauth-hint")),
        text(i18n.tr("online-connect-steps")).size(13),
        button(text(i18n.tr("online-connect-copy-link")).size(13))
            .style(button::secondary)
            .on_press(send(OnlineUiMessage::CopyLink)),
        token,
        text(i18n.tr("online-connect-store")).size(13),
        store("online-store-keyring", StoreChoice::Keyring),
        store("online-store-file", StoreChoice::File),
        store("online-store-session", StoreChoice::Session),
        muted(i18n.tr("online-connect-fairplay")),
    ]
    .spacing(8);
    if ui.token_rejected {
        body = body.push(
            text(i18n.tr("online-error-bad-token"))
                .size(13)
                .style(text::danger),
        );
    } else if let Some(line) = failure(app, i18n) {
        body = body.push(line);
    }
    if connecting {
        body = body.push(muted(i18n.tr("online-connect-connecting")));
    }
    let saved = &app.prefs.online.login;
    let mut buttons: Vec<Element<'a, Message>> = vec![cancel_button(i18n)];
    if !saved.is_empty() {
        let mut args = FluentArgs::new();
        args.set("login", saved.as_str());
        buttons.push(
            button(text(i18n.tr_args("online-connect-saved", &args)))
                .style(button::secondary)
                .on_press_maybe((!connecting).then_some(send(OnlineUiMessage::UseSaved)))
                .into(),
        );
    }
    let ready = !connecting && (!ui.token.is_blank() || ui.staged);
    buttons.push(
        button(text(i18n.tr("online-connect-button")))
            .on_press_maybe(ready.then_some(send(OnlineUiMessage::Submit)))
            .into(),
    );
    modal(Card {
        id: ONLINE_CONNECT_BOX_ID,
        title: i18n.tr("online-connect-title"),
        body: body.into(),
        buttons,
        width: 520.0,
    })
}

/// The wait for the browser: what to do there, the link to copy when the
/// program could not open it, and the way out (D3009).
fn oauth_waiting<'a>(
    page_ready: bool,
    opened: bool,
    app: &AppState,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut body = column![text(i18n.tr("online-oauth-waiting")).size(14)].spacing(8);
    if page_ready && !opened {
        body = body.push(
            text(i18n.tr("online-oauth-not-opened"))
                .size(13)
                .style(text::danger),
        );
    }
    if page_ready {
        body = body.push(
            button(text(i18n.tr("online-oauth-copy")).size(13))
                .style(button::secondary)
                .on_press(send(OnlineUiMessage::CopyOAuthLink)),
        );
    }
    if let Some(line) = failure(app, i18n) {
        body = body.push(line);
    }
    body = body.push(muted(i18n.tr("online-connect-fairplay")));
    modal(Card {
        id: ONLINE_CONNECT_BOX_ID,
        title: i18n.tr("online-connect-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("online-oauth-cancel")))
                .style(button::danger)
                .on_press(send(OnlineUiMessage::CancelOAuth))
                .into(),
        ],
        width: 480.0,
    })
}

/// "Watch a game": a link or an id.
pub fn watch<'a>(app: &AppState, ui: &'a OnlineUi, i18n: &I18n) -> Element<'a, Message> {
    let input = text_input(&i18n.tr("online-watch-placeholder"), &ui.watch_input)
        .id(ONLINE_WATCH_ID)
        .on_input(|t| send(OnlineUiMessage::WatchInput(t)))
        .on_submit(send(OnlineUiMessage::Submit))
        .padding(5);
    let mut body = column![
        text(i18n.tr("online-watch-help")).size(13),
        input,
        muted(i18n.tr("online-watch-delay")),
    ]
    .spacing(8);
    if let Some(line) = failure(app, i18n) {
        body = body.push(line);
    }
    modal(Card {
        id: ONLINE_WATCH_BOX_ID,
        title: i18n.tr("online-watch-title"),
        body: body.into(),
        buttons: vec![
            cancel_button(i18n),
            button(text(i18n.tr("online-watch-button")))
                .on_press_maybe(
                    (!ui.watch_input.trim().is_empty()).then_some(send(OnlineUiMessage::Submit)),
                )
                .into(),
        ],
        width: 480.0,
    })
}

/// The piece a premove that reaches the last rank promotes to (D3013).
pub fn promotion<'a>(i18n: &I18n) -> Element<'a, Message> {
    let pick = |key: &str, role: Role| -> Element<'a, Message> {
        button(text(i18n.tr(key)).size(14))
            .padding([6, 14])
            .style(button::secondary)
            .on_press(send(OnlineUiMessage::PromotionChoice(Some(role))))
            .into()
    };
    modal(Card {
        id: ONLINE_PROMOTION_BOX_ID,
        title: i18n.tr("online-promotion-title"),
        body: row![
            pick("online-promotion-queen", Role::Queen),
            pick("online-promotion-rook", Role::Rook),
            pick("online-promotion-bishop", Role::Bishop),
            pick("online-promotion-knight", Role::Knight),
        ]
        .spacing(8)
        .into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(OnlineUiMessage::PromotionChoice(None)))
                .into(),
        ],
        width: 440.0,
    })
}

fn time_fields<'a>(
    form: &'a TimeForm,
    i18n: &I18n,
    first_id: Option<&'static str>,
) -> Element<'a, Message> {
    let mut minutes = text_input("10", &form.minutes)
        .on_input(|t| send(OnlineUiMessage::Minutes(t)))
        .on_submit(send(OnlineUiMessage::Submit))
        .width(Length::Fixed(70.0))
        .padding(5);
    if let Some(id) = first_id {
        minutes = minutes.id(id);
    }
    let increment = text_input("0", &form.increment)
        .on_input(|t| send(OnlineUiMessage::Increment(t)))
        .on_submit(send(OnlineUiMessage::Submit))
        .width(Length::Fixed(70.0))
        .padding(5);
    row![
        text(i18n.tr("online-minutes")).size(13),
        minutes,
        text(i18n.tr("online-increment")).size(13),
        increment,
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn presets<'a>() -> Element<'a, Message> {
    let mut line = row![].spacing(6);
    for (minutes, increment) in PRESETS {
        line = line.push(
            button(text(format!("{minutes}+{increment}")).size(13))
                .padding([3, 10])
                .style(button::secondary)
                .on_press(send(OnlineUiMessage::Preset(minutes, increment))),
        );
    }
    line.into()
}

fn rated_box<'a>(form: &TimeForm, i18n: &I18n) -> Element<'a, Message> {
    checkbox(form.rated)
        .label(i18n.tr("online-rated"))
        .size(15)
        .text_size(13)
        .on_toggle(|on| send(OnlineUiMessage::Rated(on)))
        .into()
}

/// The "Standard / Chess960" selector of the seek and challenge forms.
fn variant_selector<'a>(form: &TimeForm, i18n: &I18n) -> Element<'a, Message> {
    let one = |key: &str, value: bool| -> Element<'a, Message> {
        radio(i18n.tr(key), value, Some(form.chess960), |on| {
            send(OnlineUiMessage::Chess960(on))
        })
        .size(16)
        .text_size(13)
        .into()
    };
    row![
        text(i18n.tr("online-variant")).size(13),
        one("online-variant-standard", false),
        one("online-variant-chess960", true),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

/// "Find a game": the time control, or the wait for an opponent.
pub fn seek<'a>(app: &AppState, ui: &'a OnlineUi, i18n: &I18n) -> Element<'a, Message> {
    if app.online.phase == OnlinePhase::Seeking {
        return waiting(
            ONLINE_SEEK_BOX_ID,
            i18n.tr("online-seek-title"),
            i18n.tr("online-seek-waiting"),
            i18n.tr("online-seek-cancel"),
            i18n,
        );
    }
    let valid = ui.seek_is_valid();
    let mut body = column![
        presets(),
        time_fields(&ui.seek, i18n, Some(ONLINE_MINUTES_ID)),
        variant_selector(&ui.seek, i18n),
        rated_box(&ui.seek, i18n),
    ]
    .spacing(10);
    if !valid {
        body = body.push(
            text(i18n.tr("online-seek-invalid"))
                .size(13)
                .style(text::danger),
        );
    }
    if let Some(line) = failure(app, i18n) {
        body = body.push(line);
    }
    body = body.push(muted(i18n.tr("online-assist-warning")));
    modal(Card {
        id: ONLINE_SEEK_BOX_ID,
        title: i18n.tr("online-seek-title"),
        body: body.into(),
        buttons: vec![
            cancel_button(i18n),
            button(text(i18n.tr("online-seek-button")))
                .on_press_maybe(valid.then_some(send(OnlineUiMessage::Submit)))
                .into(),
        ],
        width: 460.0,
    })
}

/// The window that waits for an opponent (or for the answer to a
/// challenge), with a single button that withdraws the request.
fn waiting<'a>(
    id: &'static str,
    title: String,
    line: String,
    cancel: String,
    i18n: &I18n,
) -> Element<'a, Message> {
    modal(Card {
        id,
        title,
        body: column![text(line).size(14), muted(i18n.tr("online-assist-warning"))]
            .spacing(8)
            .into(),
        buttons: vec![
            button(text(cancel))
                .style(button::danger)
                .on_press(send(OnlineUiMessage::CancelWait))
                .into(),
        ],
        width: 420.0,
    })
}

fn color_radio<'a>(
    i18n: &I18n,
    key: &str,
    color: OnlineColor,
    selected: OnlineColor,
) -> Element<'a, Message> {
    radio(i18n.tr(key), color, Some(selected), |c| {
        send(OnlineUiMessage::Color(c))
    })
    .size(16)
    .text_size(13)
    .into()
}

/// "Challenge a user".
pub fn challenge<'a>(app: &AppState, ui: &'a OnlineUi, i18n: &I18n) -> Element<'a, Message> {
    if app.online.phase == OnlinePhase::Challenging {
        let mut args = FluentArgs::new();
        args.set("user", ui.challenge.user.trim());
        return waiting(
            ONLINE_CHALLENGE_BOX_ID,
            i18n.tr("online-challenge-title"),
            i18n.tr_args("online-challenge-waiting", &args),
            i18n.tr("online-challenge-cancel"),
            i18n,
        );
    }
    let user = text_input(&i18n.tr("online-challenge-user"), &ui.challenge.user)
        .id(ONLINE_USER_ID)
        .on_input(|t| send(OnlineUiMessage::ChallengeUser(t)))
        .on_submit(send(OnlineUiMessage::Submit))
        .padding(5);
    let valid = ui.challenge_is_valid();
    let selected = ui.challenge.color;
    let mut body = column![
        user,
        presets(),
        time_fields(&ui.challenge.time, i18n, None),
        variant_selector(&ui.challenge.time, i18n),
        rated_box(&ui.challenge.time, i18n),
        row![
            color_radio(i18n, "online-color-random", OnlineColor::Random, selected),
            color_radio(i18n, "online-color-white", OnlineColor::White, selected),
            color_radio(i18n, "online-color-black", OnlineColor::Black, selected),
        ]
        .spacing(12),
    ]
    .spacing(10);
    if ui.challenge.time.chess960 {
        body = body.push(
            row![
                text(i18n.tr("online-challenge-start")).size(13),
                text_input(&i18n.tr("online-challenge-start-hint"), &ui.challenge.start)
                    .on_input(|t| send(OnlineUiMessage::ChallengeStart(t)))
                    .on_submit(send(OnlineUiMessage::Submit))
                    .width(Length::Fixed(110.0))
                    .padding(5),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        if !ui.challenge_start_is_valid() {
            body = body.push(
                text(i18n.tr("online-challenge-start-invalid"))
                    .size(13)
                    .style(text::danger),
            );
        }
    }
    if !valid && !ui.challenge.user.trim().is_empty() && ui.challenge_start_is_valid() {
        body = body.push(
            text(i18n.tr("online-challenge-invalid"))
                .size(13)
                .style(text::danger),
        );
    }
    if let Some(line) = failure(app, i18n) {
        body = body.push(line);
    }
    body = body.push(muted(i18n.tr("online-assist-warning")));
    modal(Card {
        id: ONLINE_CHALLENGE_BOX_ID,
        title: i18n.tr("online-challenge-title"),
        body: body.into(),
        buttons: vec![
            cancel_button(i18n),
            button(text(i18n.tr("online-challenge-button")))
                .on_press_maybe(valid.then_some(send(OnlineUiMessage::Submit)))
                .into(),
        ],
        width: 480.0,
    })
}

/// The challenges received, with Accept and Decline.
pub fn incoming<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let mut list = column![].spacing(8);
    if app.online.incoming.is_empty() {
        list = list.push(muted(i18n.tr("online-incoming-empty")));
    }
    for challenge in &app.online.incoming {
        let mut who = challenge.from.clone();
        if let Some(title) = &challenge.title {
            who = format!("{title} {who}");
        }
        if let Some(rating) = challenge.rating {
            who = format!("{who} ({rating})");
        }
        let mut args = FluentArgs::new();
        args.set("who", who);
        args.set("time", challenge.time.as_str());
        let key = if challenge.rated {
            "online-incoming-rated"
        } else {
            "online-incoming-casual"
        };
        let mut line = column![text(i18n.tr_args(key, &args)).size(13)].spacing(2);
        if challenge.chess960 {
            line = line.push(muted(i18n.tr("online-incoming-chess960")));
        }
        if !challenge.playable {
            line = line.push(muted(i18n.tr("online-incoming-unplayable")));
        }
        list = list.push(
            row![
                container(line).width(Length::Fill),
                button(text(i18n.tr("online-incoming-accept")).size(13))
                    .padding([3, 10])
                    .on_press_maybe(
                        challenge
                            .playable
                            .then(|| send(OnlineUiMessage::Accept(challenge.id.clone()))),
                    ),
                button(text(i18n.tr("online-incoming-decline")).size(13))
                    .padding([3, 10])
                    .style(button::secondary)
                    .on_press(send(OnlineUiMessage::Decline(challenge.id.clone()))),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    let mut body = column![list].spacing(8);
    if let Some(line) = failure(app, i18n) {
        body = body.push(line);
    }
    modal(Card {
        id: ONLINE_INCOMING_BOX_ID,
        title: i18n.tr("online-incoming-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("online-close")))
                .style(button::primary)
                .on_press(send(OnlineUiMessage::Close))
                .into(),
        ],
        width: 520.0,
    })
}

/// The confirmation of Resign, of removing the lock and of quitting during
/// a game: Yes carries it out, No (and Esc, and Enter) goes back (D1478).
pub fn confirm<'a>(kind: OnlineConfirm, i18n: &I18n) -> Element<'a, Message> {
    let (title, body) = match kind {
        OnlineConfirm::Resign => ("online-confirm-resign-title", "online-confirm-resign"),
        OnlineConfirm::RemoveLock => ("online-confirm-unlock-title", "online-confirm-unlock"),
        OnlineConfirm::Quit => ("online-confirm-quit-title", "online-confirm-quit"),
    };
    modal(Card {
        id: ONLINE_CONFIRM_BOX_ID,
        title: i18n.tr(title),
        body: text(i18n.tr(body)).size(13).into(),
        buttons: vec![
            button(text(i18n.tr("online-no")))
                .style(button::primary)
                .on_press(send(OnlineUiMessage::Confirm(false)))
                .into(),
            button(text(i18n.tr("online-yes")))
                .style(button::danger)
                .on_press(send(OnlineUiMessage::Confirm(true)))
                .into(),
        ],
        width: 460.0,
    })
}
