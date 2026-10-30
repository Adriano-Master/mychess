//! The FICS window (spec M8-9c, D2601-D2628): connection, seek graph,
//! challenges, observed games, offers, console and the settings of the
//! play. One window; the rules live in `mychess_app::fics`, this file keeps
//! only what is typed and the strips of the board.

use std::fmt;
use std::time::Instant;

use fluent_bundle::FluentArgs;
use iced::widget::{
    Space, button, canvas, column, container, radio, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length};
use mychess_app::fics::{
    AutoPromote, ColorChoice, FicsGame, FicsMessage, FicsPrefs, LineKind, Offer, Phase, Role,
    SeekSpec,
};
use mychess_app::{AppState, Message as AppMessage};
use mychess_core::{Color as Side, Square};

use crate::app_api;
use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::check;
use crate::windows::dialog_ui::Outcome;
use crate::windows::fics_graph::{Dot, SeekPlot, dot_of};
use crate::windows::online_ui::{PremoveRule, Secret, shown_is_current};
use crate::windows::play_dialog::shown_seconds;

/// Id of the window.
pub const FICS_BOX_ID: &str = "fics-box";
/// Id of the field of the console.
pub const FICS_COMMAND_ID: &str = "fics-command";
/// Id of the field of the handle.
pub const FICS_NAME_ID: &str = "fics-name";

/// Most console lines drawn (the state keeps more).
const SHOWN_LINES: usize = 300;

/// An event of the window that only the front-end understands.
#[derive(Clone, PartialEq)]
pub enum FicsUiMessage {
    /// Open the window (the menu).
    Open,
    /// The console field changed.
    Command(String),
    /// Enter in the console field.
    SubmitCommand,
    /// The handle field changed.
    Name(String),
    /// The password field changed.
    Password(Secret),
    /// "Connect".
    Connect,
    /// Minutes of the seek form.
    Minutes(String),
    /// Increment of the seek form.
    Increment(String),
    /// Rated game.
    Rated(bool),
    /// The player of the challenge.
    Player(String),
    /// The game or player to observe, follow or examine.
    Target(String),
    /// "Find an opponent".
    SeekNow,
    /// "Challenge".
    ChallengeNow,
    /// "Observe".
    ObserveNow,
    /// "Follow".
    FollowNow,
    /// "Examine".
    ExamineNow,
    /// A dot of the graph was picked.
    SelectSeek(Option<u32>),
    /// "Play" the dot picked.
    PlaySelected,
    /// The clocks moved.
    Tick(Instant),
    /// A premove asked for by the board.
    Premove(crate::widgets::board::premove::PremoveEvent),
    /// The command Resign (button, menu, action): asks first unless the
    /// preference says not to (D3109).
    Resign,
    /// The answer to the question "Resign?" (`true` = yes).
    ConfirmResign(bool),
}

impl fmt::Debug for FicsUiMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Never the text of a password.
            FicsUiMessage::Password(_) => f.write_str("Password(<redacted>)"),
            other => write!(f, "{}", variant_name(other)),
        }
    }
}

fn variant_name(message: &FicsUiMessage) -> &'static str {
    match message {
        FicsUiMessage::Open => "Open",
        FicsUiMessage::Command(_) => "Command",
        FicsUiMessage::SubmitCommand => "SubmitCommand",
        FicsUiMessage::Name(_) => "Name",
        FicsUiMessage::Password(_) => "Password",
        FicsUiMessage::Connect => "Connect",
        FicsUiMessage::Minutes(_) => "Minutes",
        FicsUiMessage::Increment(_) => "Increment",
        FicsUiMessage::Rated(_) => "Rated",
        FicsUiMessage::Player(_) => "Player",
        FicsUiMessage::Target(_) => "Target",
        FicsUiMessage::SeekNow => "SeekNow",
        FicsUiMessage::ChallengeNow => "ChallengeNow",
        FicsUiMessage::ObserveNow => "ObserveNow",
        FicsUiMessage::FollowNow => "FollowNow",
        FicsUiMessage::ExamineNow => "ExamineNow",
        FicsUiMessage::SelectSeek(_) => "SelectSeek",
        FicsUiMessage::PlaySelected => "PlaySelected",
        FicsUiMessage::Tick(_) => "Tick",
        FicsUiMessage::Premove(_) => "Premove",
        FicsUiMessage::Resign => "Resign",
        FicsUiMessage::ConfirmResign(_) => "ConfirmResign",
    }
}

/// What is typed in the window and the strips of the board.
pub struct FicsUi {
    /// The console field.
    pub command: String,
    /// The handle; empty is a guest.
    pub name: String,
    /// The password (redacted in `Debug`).
    pub password: Secret,
    /// Minutes of the forms.
    pub minutes: String,
    /// Increment of the forms.
    pub increment: String,
    /// Rated game.
    pub rated: bool,
    /// The player to challenge.
    pub player: String,
    /// The game or player to observe.
    pub target: String,
    /// The advertisement picked in the graph.
    pub selected: Option<u32>,
    /// The instant of the last event (views never read the clock).
    pub now: Instant,
    /// The strips of the board with the clocks of the game.
    pub strips: Option<mychess_app::PlayerStrips>,
    /// The premove rule for the board.
    pub premove_rule: PremoveRule,
    /// The question "Resign?" is on screen (D3109).
    pub confirm_resign: bool,
    loaded: bool,
}

impl Default for FicsUi {
    fn default() -> FicsUi {
        FicsUi {
            command: String::new(),
            name: String::new(),
            password: Secret::default(),
            minutes: "5".to_owned(),
            increment: "0".to_owned(),
            rated: false,
            player: String::new(),
            target: String::new(),
            selected: None,
            now: Instant::now(),
            strips: None,
            premove_rule: PremoveRule::default(),
            confirm_resign: false,
            loaded: false,
        }
    }
}

impl fmt::Debug for FicsUi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FicsUi").finish_non_exhaustive()
    }
}

fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).take(3).collect()
}

fn fics(message: FicsMessage) -> AppMessage {
    AppMessage::Fics(message)
}

fn ui(message: FicsUiMessage) -> Message {
    Message::Fics(message)
}

fn app_msg(message: FicsMessage) -> Message {
    Message::App(fics(message))
}

/// The application message "Close".
pub fn cancel_message() -> AppMessage {
    fics(FicsMessage::CloseWindow)
}

impl FicsUi {
    /// Fills the forms from the saved settings, once per session.
    pub fn load(&mut self, prefs: &FicsPrefs) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.name.clone_from(&prefs.login);
        self.minutes = prefs.minutes.to_string();
        self.increment = prefs.increment.to_string();
        self.rated = prefs.rated;
    }

    fn times(&self) -> (u32, u32) {
        (
            self.minutes.parse().unwrap_or(0).min(600),
            self.increment.parse().unwrap_or(0).min(600),
        )
    }

    /// Takes an event; the messages for the application are in the outcome.
    pub fn update(&mut self, message: FicsUiMessage, app: &AppState) -> Outcome {
        match message {
            FicsUiMessage::Command(t) => self.command = t,
            FicsUiMessage::SubmitCommand => {
                let line = std::mem::take(&mut self.command);
                if !line.trim().is_empty() {
                    return Outcome::send(fics(FicsMessage::Console(line)));
                }
            }
            FicsUiMessage::Name(t) => {
                self.name = t
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .take(17)
                    .collect();
            }
            FicsUiMessage::Password(p) => self.password = p,
            FicsUiMessage::Connect => return self.connect(app),
            FicsUiMessage::Minutes(t) => self.minutes = digits(&t),
            FicsUiMessage::Increment(t) => self.increment = digits(&t),
            FicsUiMessage::Rated(v) => self.rated = v,
            FicsUiMessage::Player(t) => self.player = t,
            FicsUiMessage::Target(t) => self.target = t,
            FicsUiMessage::SeekNow => {
                let (minutes, increment) = self.times();
                return Outcome::send(fics(FicsMessage::Seek(SeekSpec {
                    minutes,
                    increment,
                    rated: self.rated,
                    color: ColorChoice::Any,
                    manual: false,
                    range: None,
                })));
            }
            FicsUiMessage::ChallengeNow => {
                let (minutes, increment) = self.times();
                return Outcome::send(fics(FicsMessage::Challenge {
                    player: self.player.trim().to_owned(),
                    minutes,
                    increment,
                    rated: self.rated,
                    color: ColorChoice::Any,
                }));
            }
            FicsUiMessage::ObserveNow => {
                return Outcome::send(fics(FicsMessage::Observe(self.target.trim().to_owned())));
            }
            FicsUiMessage::FollowNow => {
                return Outcome::send(fics(FicsMessage::Follow(self.target.trim().to_owned())));
            }
            FicsUiMessage::ExamineNow => {
                let who = self.target.trim();
                return Outcome::send(fics(FicsMessage::Examine(
                    (!who.is_empty()).then(|| who.to_owned()),
                )));
            }
            FicsUiMessage::SelectSeek(index) => self.selected = index,
            FicsUiMessage::PlaySelected => {
                if let Some(index) = self.selected {
                    return Outcome::send(fics(FicsMessage::PlaySeek(index)));
                }
            }
            FicsUiMessage::Resign => return self.resign(app),
            FicsUiMessage::ConfirmResign(yes) => {
                let was = std::mem::take(&mut self.confirm_resign);
                // The game may have ended while the question was open.
                if yes && was && app.fics().playing() {
                    return Outcome::send(fics(FicsMessage::Resign));
                }
            }
            // The shell handles these before they get here.
            FicsUiMessage::Open | FicsUiMessage::Tick(_) | FicsUiMessage::Premove(_) => {}
        }
        Outcome::default()
    }

    /// Resigning is irreversible: it asks first, as the other modes do
    /// (D3109); nothing without a game of the user on.
    fn resign(&mut self, app: &AppState) -> Outcome {
        if !app.fics().playing() {
            return Outcome::default();
        }
        if app.prefs.online.confirm_resign {
            self.confirm_resign = true;
            Outcome::default()
        } else {
            Outcome::send(fics(FicsMessage::Resign))
        }
    }

    /// Stages the password in the hub (where it stays) and asks to connect.
    fn connect(&mut self, app: &AppState) -> Outcome {
        let name = self.name.trim().to_owned();
        let named = !name.is_empty();
        // Nothing staged before survives into this attempt (D2698).
        app.fics.hub.clear_staged();
        if named && !self.password.is_blank() {
            // A password the hub refuses (control characters) is dropped:
            // the server will say the login failed.
            drop(app.fics.hub.stage_password(self.password.expose()));
        }
        self.password.clear();
        Outcome::send(fics(FicsMessage::Connect(
            mychess_app::fics::ConnectRequest {
                name: named.then_some(name),
            },
        )))
    }

    /// Brings the visual state up to date with the application.
    pub fn sync(&mut self, app: &AppState) {
        let state = app.fics();
        // The game ended (or the link fell) while the question was open.
        if !state.playing() {
            self.confirm_resign = false;
        }
        if state.phase == Phase::Disconnected
            || self.selected.is_some_and(|i| state.seeks.get(i).is_none())
        {
            self.selected = None;
        }
        self.rebuild_strips(app);
        self.rebuild_premove_rule(app);
    }

    fn clocked(app: &AppState) -> Option<&FicsGame> {
        app.fics()
            .editor_game()
            .filter(|g| g.role != Role::Examining)
    }

    fn rebuild_strips(&mut self, app: &AppState) {
        let Some(game) = Self::clocked(app) else {
            self.strips = None;
            return;
        };
        let mut strips = app_api::player_strips(app).clone();
        strips.clock_slot = true;
        strips.white.clock = Some(shown_seconds(game.shown(Side::White, self.now), true));
        strips.black.clock = Some(shown_seconds(game.shown(Side::Black, self.now), true));
        self.strips = Some(strips);
    }

    /// Redraws the clocks at `now`; whether anything changed.
    pub fn refresh_clocks(&mut self, app: &AppState) -> bool {
        let before = self.strips.clone();
        self.rebuild_strips(app);
        self.strips != before
    }

    fn rebuild_premove_rule(&mut self, app: &AppState) {
        let rule = app
            .fics()
            .game
            .as_ref()
            .filter(|g| g.is_live_player() && !g.my_turn())
            .filter(|_| app.fics().prefs.premove)
            .filter(|g| shown_is_current(app, g.position().zobrist()))
            .and_then(|g| {
                let position = g.position().clone();
                let color = g.my_color?;
                Some(Box::new(move |from: Square, to: Square| {
                    mychess_app::online::is_pseudo_legal(&position, color, from, to)
                })
                    as Box<dyn Fn(Square, Square) -> bool + Send + Sync>)
            });
        self.premove_rule = PremoveRule::new(rule);
    }
}

/// Whether the clocks of a game are running (a tick is useful).
pub fn wants_ticks(app: &AppState) -> bool {
    FicsUi::clocked(app).is_some_and(|g| g.result.is_none()) || app.fics().needs_ticks()
}

fn muted<'a>(content: String) -> Element<'a, Message> {
    text(content).size(12).style(theme::muted_text).into()
}

fn tr_with(i18n: &I18n, key: &str, name: &str, value: &str) -> String {
    let mut args = FluentArgs::new();
    args.set(name, value);
    i18n.tr_args(key, &args)
}

fn field<'a>(
    placeholder: &str,
    value: &str,
    on_input: impl Fn(String) -> FicsUiMessage + 'a,
) -> iced::widget::TextInput<'a, Message> {
    text_input(placeholder, value)
        .on_input(move |t| ui(on_input(t)))
        .padding(4)
        .size(13)
}

fn small<'a>(label: String, press: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(13))
        .style(button::secondary)
        .on_press_maybe(press)
        .into()
}

/// Id of the question "Resign?".
pub const FICS_CONFIRM_BOX_ID: &str = "fics-confirm-box";

/// The question "Resign?": No (and Esc, and Enter) goes back, Yes resigns
/// (D3109), like the confirmation of the other modes (D1478).
pub fn confirm_resign<'a>(i18n: &I18n) -> Element<'a, Message> {
    modal(Card {
        id: FICS_CONFIRM_BOX_ID,
        title: i18n.tr("fics-confirm-resign-title"),
        body: text(i18n.tr("fics-confirm-resign")).size(13).into(),
        buttons: vec![
            button(text(i18n.tr("fics-no")))
                .style(button::primary)
                .on_press(ui(FicsUiMessage::ConfirmResign(false)))
                .into(),
            button(text(i18n.tr("fics-yes")))
                .style(button::danger)
                .on_press(ui(FicsUiMessage::ConfirmResign(true)))
                .into(),
        ],
        width: 460.0,
    })
}

/// The window.
pub fn view<'a>(app: &AppState, fics_ui: &'a FicsUi, i18n: &I18n) -> Element<'a, Message> {
    let state = app.fics();
    let body: Element<'a, Message> = if state.phase == Phase::LoggedIn {
        column![
            status_line(app, i18n),
            row![
                column![seek_graph(app, fics_ui, i18n), offers(app, i18n)]
                    .spacing(8)
                    .width(Length::FillPortion(3)),
                column![
                    forms(fics_ui, i18n),
                    observed(app, i18n),
                    settings(app, i18n)
                ]
                .spacing(8)
                .width(Length::FillPortion(2)),
            ]
            .spacing(12),
            console(app, fics_ui, i18n),
        ]
        .spacing(8)
        .into()
    } else {
        column![login(app, fics_ui, i18n), console(app, fics_ui, i18n)]
            .spacing(8)
            .into()
    };
    let buttons: Vec<Element<'a, Message>> = vec![
        button(text(i18n.tr("fics-close")))
            .style(button::secondary)
            .on_press(Message::App(cancel_message()))
            .into(),
        button(text(i18n.tr("fics-disconnect")))
            .style(button::danger)
            .on_press_maybe(
                state
                    .is_connected()
                    .then(|| app_msg(FicsMessage::Disconnect)),
            )
            .into(),
    ];
    modal(Card {
        id: FICS_BOX_ID,
        title: i18n.tr("fics-title"),
        body,
        buttons,
        width: 920.0,
    })
}

fn login<'a>(app: &AppState, ui_state: &'a FicsUi, i18n: &I18n) -> Element<'a, Message> {
    let connecting = app.fics().phase == Phase::Connecting;
    let name = field(
        &i18n.tr("fics-name-placeholder"),
        &ui_state.name,
        FicsUiMessage::Name,
    )
    .id(FICS_NAME_ID)
    .on_submit(ui(FicsUiMessage::Connect));
    let password = text_input(
        &i18n.tr("fics-password-placeholder"),
        ui_state.password.expose(),
    )
    .secure(true)
    .on_input_maybe(
        (!ui_state.name.is_empty())
            .then_some(|t: String| ui(FicsUiMessage::Password(Secret::new(t)))),
    )
    .on_submit(ui(FicsUiMessage::Connect))
    .padding(4)
    .size(13);
    column![
        text(i18n.tr("fics-login-hint")).size(13),
        name,
        password,
        muted(i18n.tr("fics-password-warning")),
        muted(i18n.tr("fics-fairplay")),
        button(text(i18n.tr(if ui_state.name.is_empty() {
            "fics-connect-guest"
        } else {
            "fics-connect"
        })))
        .on_press_maybe((!connecting).then(|| ui(FicsUiMessage::Connect))),
        if connecting {
            muted(i18n.tr("fics-connecting"))
        } else {
            muted(String::new())
        },
    ]
    .spacing(6)
    .into()
}

fn status_line<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let state = app.fics();
    let me = state.me.clone().unwrap_or_default();
    let mut line = tr_with(i18n, "fics-status-as", "name", &me);
    if let Some(ms) = state.ping_ms.or_else(|| {
        state
            .latency
            .map(|d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX))
    }) {
        line.push_str("  |  ");
        line.push_str(&tr_with(i18n, "fics-ping", "ms", &ms.to_string()));
    }
    if let Some(who) = &state.following {
        line.push_str("  |  ");
        line.push_str(&tr_with(i18n, "fics-following", "name", who));
    }
    let mut content = column![text(line).size(13)].spacing(2);
    if app.assist_locked() {
        content = content.push(
            text(i18n.tr("online-assist-off"))
                .size(12)
                .style(text::danger),
        );
    }
    content.into()
}

fn seek_graph<'a>(app: &AppState, ui_state: &'a FicsUi, i18n: &I18n) -> Element<'a, Message> {
    let seeks = &app.fics().seeks;
    let dots: Vec<Dot> = seeks.ads().iter().map(dot_of).collect();
    let plot = SeekPlot {
        dots,
        selected: ui_state.selected,
    };
    let info = match ui_state.selected.and_then(|i| seeks.get(i)) {
        Some(seek) => {
            let mut args = FluentArgs::new();
            args.set("name", seek.name.as_str());
            args.set(
                "rating",
                seek.rating
                    .map_or_else(|| "-".to_owned(), |r| r.to_string()),
            );
            args.set("minutes", i64::from(seek.minutes));
            args.set("increment", i64::from(seek.increment));
            args.set("kind", seek.kind.as_str());
            args.set(
                "rated",
                i18n.tr(if seek.rated {
                    "fics-rated"
                } else {
                    "fics-unrated"
                }),
            );
            i18n.tr_args("fics-seek-info", &args)
        }
        None => i18n.tr("fics-seek-pick"),
    };
    let playable = ui_state
        .selected
        .and_then(|i| seeks.get(i))
        .is_some_and(mychess_app::fics::is_playable)
        && !app.fics().playing();
    let mut count = FluentArgs::new();
    count.set("count", i64::try_from(seeks.len()).unwrap_or(i64::MAX));
    column![
        text(i18n.tr_args("fics-seeks-title", &count)).size(13),
        canvas(plot)
            .width(Length::Fill)
            .height(Length::Fixed(200.0)),
        row![
            text(info).size(12).width(Length::Fill),
            button(text(i18n.tr("fics-play")).size(13))
                .on_press_maybe(playable.then(|| ui(FicsUiMessage::PlaySelected))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(4)
    .into()
}

fn offers<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let list = &app.fics().offers;
    if list.is_empty() {
        return Space::new().into();
    }
    let mut col = column![text(i18n.tr("fics-offers")).size(13)].spacing(4);
    for offer in list {
        let (label, accept, decline) = match offer {
            Offer::Match {
                from,
                rating,
                rated,
                kind,
                minutes,
                increment,
            } => {
                let mut args = FluentArgs::new();
                args.set("name", from.as_str());
                args.set(
                    "rating",
                    rating.map_or_else(|| "-".to_owned(), |r| r.to_string()),
                );
                args.set("minutes", i64::from(*minutes));
                args.set("increment", i64::from(*increment));
                args.set("kind", kind.as_str());
                args.set(
                    "rated",
                    i18n.tr(if *rated { "fics-rated" } else { "fics-unrated" }),
                );
                (
                    i18n.tr_args("fics-offer-match", &args),
                    Some(app_msg(FicsMessage::Accept(Some(from.clone())))),
                    Some(app_msg(FicsMessage::Decline(Some(from.clone())))),
                )
            }
            Offer::Draw { from } => (
                tr_with(i18n, "fics-offer-draw", "name", from),
                Some(app_msg(FicsMessage::OfferDraw)),
                None,
            ),
            Offer::Takeback { from, plies } => {
                let mut args = FluentArgs::new();
                args.set("name", from.as_str());
                args.set("plies", i64::from(*plies));
                (i18n.tr_args("fics-offer-takeback", &args), None, None)
            }
            Offer::Abort { from } => (
                tr_with(i18n, "fics-offer-abort", "name", from),
                Some(app_msg(FicsMessage::Abort)),
                None,
            ),
            _ => continue,
        };
        col = col.push(
            row![
                text(label).size(12).width(Length::Fill),
                small(i18n.tr("fics-accept"), accept),
                small(i18n.tr("fics-decline"), decline),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    col.into()
}

fn forms<'a>(ui_state: &'a FicsUi, i18n: &I18n) -> Element<'a, Message> {
    let times = row![
        field("5", &ui_state.minutes, FicsUiMessage::Minutes).width(Length::Fixed(56.0)),
        text("+").size(13),
        field("0", &ui_state.increment, FicsUiMessage::Increment).width(Length::Fixed(56.0)),
        check(i18n.tr("fics-rated"), ui_state.rated, true, |v| {
            ui(FicsUiMessage::Rated(v))
        }),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let challenge_ready = ui_state.player.trim().len() >= 2;
    let target_ready = !ui_state.target.trim().is_empty();
    column![
        text(i18n.tr("fics-find")).size(13),
        times,
        row![
            small(i18n.tr("fics-seek"), Some(ui(FicsUiMessage::SeekNow))),
            small(i18n.tr("fics-unseek"), Some(app_msg(FicsMessage::Unseek))),
        ]
        .spacing(6),
        field(
            &i18n.tr("fics-player-placeholder"),
            &ui_state.player,
            FicsUiMessage::Player
        )
        .on_submit(ui(FicsUiMessage::ChallengeNow)),
        small(
            i18n.tr("fics-challenge"),
            challenge_ready.then(|| ui(FicsUiMessage::ChallengeNow)),
        ),
        field(
            &i18n.tr("fics-target-placeholder"),
            &ui_state.target,
            FicsUiMessage::Target
        )
        .on_submit(ui(FicsUiMessage::ObserveNow)),
        row![
            small(
                i18n.tr("fics-observe"),
                target_ready.then(|| ui(FicsUiMessage::ObserveNow))
            ),
            small(
                i18n.tr("fics-follow"),
                target_ready.then(|| ui(FicsUiMessage::FollowNow))
            ),
            small(i18n.tr("fics-examine"), Some(ui(FicsUiMessage::ExamineNow))),
        ]
        .spacing(6),
    ]
    .spacing(6)
    .into()
}

fn observed<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let state = app.fics();
    if state.observed.is_empty() {
        return Space::new().into();
    }
    let mut args = FluentArgs::new();
    args.set("count", i64::try_from(state.observed.len()).unwrap_or(0));
    args.set(
        "max",
        i64::try_from(mychess_app::fics::MAX_OBSERVED).unwrap_or(0),
    );
    let mut col = column![text(i18n.tr_args("fics-observed", &args)).size(13)].spacing(4);
    for game in &state.observed {
        let label = format!("#{} {} - {}", game.number, game.white, game.black);
        col = col.push(
            row![
                text(label).size(12).width(Length::Fill),
                small(
                    i18n.tr("fics-show"),
                    (state.game.is_none() && state.shown != Some(game.number))
                        .then(|| app_msg(FicsMessage::Show(game.number))),
                ),
                small(
                    i18n.tr("fics-unobserve"),
                    Some(app_msg(FicsMessage::Unobserve(game.number))),
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    col.into()
}

fn settings<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let state = app.fics();
    let prefs = state.prefs.clone();
    let set = |next: FicsPrefs| app_msg(FicsMessage::SetPrefs(Box::new(next)));
    let toggle = |key: &str, value: bool, apply: fn(&mut FicsPrefs, bool)| {
        let base = prefs.clone();
        check(i18n.tr(key), value, true, move |v| {
            let mut next = base.clone();
            apply(&mut next, v);
            set(next)
        })
    };
    let promote = |key: &str, choice: AutoPromote| -> Element<'a, Message> {
        let base = prefs.clone();
        radio(i18n.tr(key), choice, Some(prefs.auto_promote), move |c| {
            let mut next = base.clone();
            next.auto_promote = c;
            set(next)
        })
        .size(14)
        .text_size(12)
        .into()
    };
    column![
        text(i18n.tr("fics-settings")).size(13),
        toggle("fics-premove", prefs.premove, |p, v| p.premove = v),
        toggle("fics-store-emt", prefs.store_emt, |p, v| p.store_emt = v),
        toggle("fics-store-clk", prefs.store_clk, |p, v| p.store_clk = v),
        check(i18n.tr("fics-lectures"), state.lectures, true, |v| {
            app_msg(FicsMessage::Lectures(v))
        }),
        text(i18n.tr("fics-auto-promote")).size(12),
        row![
            promote("fics-promote-ask", AutoPromote::Ask),
            promote("fics-promote-queen", AutoPromote::Queen),
            promote("fics-promote-rook", AutoPromote::Rook),
            promote("fics-promote-bishop", AutoPromote::Bishop),
            promote("fics-promote-knight", AutoPromote::Knight),
        ]
        .spacing(6),
    ]
    .spacing(4)
    .into()
}

fn console<'a>(app: &AppState, ui_state: &'a FicsUi, i18n: &I18n) -> Element<'a, Message> {
    let console = &app.fics().console;
    let skip = console.len().saturating_sub(SHOWN_LINES);
    let mut lines = column![].spacing(1);
    for line in console.lines().skip(skip) {
        let t = text(line.text.clone()).size(12).font(iced::Font::MONOSPACE);
        lines = lines.push(match line.kind {
            LineKind::Input => t.style(theme::accent_text),
            LineKind::System => t.style(theme::muted_text),
            _ => t,
        });
    }
    let input = field(
        &i18n.tr("fics-command-placeholder"),
        &ui_state.command,
        FicsUiMessage::Command,
    )
    .id(FICS_COMMAND_ID)
    .on_submit(ui(FicsUiMessage::SubmitCommand));
    column![
        container(scrollable(lines).anchor_bottom().width(Length::Fill))
            .height(Length::Fixed(150.0))
            .padding(4)
            .style(theme::pane_body),
        row![
            input.width(Length::Fill),
            small(
                i18n.tr("fics-clear"),
                Some(app_msg(FicsMessage::ClearConsole))
            ),
        ]
        .spacing(6),
    ]
    .spacing(4)
    .into()
}
