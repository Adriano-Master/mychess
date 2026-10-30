//! The navigation bar under the board (spec M2-4, D199, D202; Scid
//! `.main.button`): start, back, forward, end, variation controls, trial
//! mode, flip, autoplay and the field where moves are typed.
//!
//! Which buttons are on comes from the application ([`NavState`]). The
//! field shows the application's move-entry buffer; every edit is turned
//! into the keystrokes the buffer understands, so typing in the field and
//! typing over the board are the same thing (D199).

use fluent_bundle::FluentArgs;
use iced::widget::{button, container, responsive, row, text, text_input, tooltip};
use iced::{Element, Length, Padding};
use iced_aw::menu::{Item, Menu, MenuBar};
use mychess_app::engines::EngineRunState;
use mychess_app::{
    Action, AppState, GameMessage, Message as AppMessage, ModeMessage, Nav, NavState, Shortcuts,
};

use crate::i18n::I18n;
use crate::keymap::{Platform, binding_label};
use crate::shell::{Message, UiMessage};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::right_click::RightClick;
use crate::windows::dialog_ui::DialogMessage;

/// Id of the move field.
pub const MOVE_FIELD_ID: &str = "move-entry";

fn game(message: GameMessage) -> Message {
    Message::App(AppMessage::Game(message))
}

/// The keystrokes that turn the buffer `old` into `new`: characters added at
/// the end, backspaces for those removed at the end, or a clear and retype
/// when the text changed elsewhere.
pub fn entry_edit(old: &str, new: &str) -> Vec<GameMessage> {
    if let Some(added) = new.strip_prefix(old) {
        return added.chars().map(GameMessage::MoveEntryChar).collect();
    }
    if let Some(removed) = old.strip_prefix(new) {
        return removed
            .chars()
            .map(|_| GameMessage::MoveEntryBackspace)
            .collect();
    }
    let mut out = vec![GameMessage::MoveEntryClear];
    out.extend(new.chars().map(GameMessage::MoveEntryChar));
    out
}

/// The move-entry field: shows the buffer of the application and turns every
/// edit into keystrokes (see [`entry_edit`]).
pub fn move_field<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let entry = app.session.move_entry();
    let current = entry.text().to_owned();
    text_input(&i18n.tr("move-entry-placeholder"), entry.text())
        .id(MOVE_FIELD_ID)
        .on_input(move |new| {
            // One message per keystroke, in order.
            let keys = entry_edit(&current, &new);
            Message::Dialog(DialogMessage::MoveEntryKeys(keys))
        })
        .on_submit(game(GameMessage::MoveEntryCommit))
        .padding(4)
        .width(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------
// The icon command bar (spec M2-5, D225 to D228)
// ---------------------------------------------------------------------

/// Id of the start button.
pub const NAV_START: &str = "nav-start";
/// Id of the back button.
pub const NAV_BACK: &str = "nav-back";
/// Id of the play/pause button.
pub const NAV_PLAY: &str = "nav-play";
/// Id of the forward button.
pub const NAV_FORWARD: &str = "nav-forward";
/// Id of the end button.
pub const NAV_END: &str = "nav-end";
/// Id of the add-variation button.
pub const NAV_ADD_VARIATION: &str = "nav-add-variation";
/// Id of the exit-variation button.
pub const NAV_EXIT_VARIATION: &str = "nav-exit-variation";
/// Id of the choose-variation button.
pub const NAV_ENTER_VARIATION: &str = "nav-enter-variation";
/// Id of the trial-mode button.
pub const NAV_TRIAL: &str = "nav-trial";
/// Id of the undo button.
pub const NAV_UNDO: &str = "nav-undo";
/// Id of the redo button.
pub const NAV_REDO: &str = "nav-redo";
/// Id of the flip button.
pub const NAV_FLIP: &str = "nav-flip";
/// Id of the "more commands" button of the narrow layouts.
pub const NAV_MORE: &str = "nav-more";

/// Which row a command belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    /// The five large buttons: start, back, play, forward, end.
    Primary,
    /// The small buttons of the second row.
    Secondary,
}

/// A command of the bar as data: what it looks like, whether it is on and
/// what it sends (D226). [`buttons`] builds them from the [`NavState`], so
/// the bar is tested without a window.
#[derive(Debug, Clone)]
pub struct NavButton {
    /// Stable id of the widget (`nav-start`, ...).
    pub id: &'static str,
    /// The icon.
    pub icon: Icon,
    /// Fluent key of the label (also the first part of the tooltip).
    pub label_key: &'static str,
    /// The action whose first shortcut the tooltip shows.
    pub action: Option<Action>,
    /// What a click sends; `None` while the button is off.
    pub message: Option<Message>,
    /// Whether the button reacts.
    pub enabled: bool,
    /// Whether it is switched on (trial mode, autoplay running).
    pub toggled: bool,
    /// The row.
    pub size: ButtonSize,
}

impl NavButton {
    fn new(
        id: &'static str,
        icon: Icon,
        label_key: &'static str,
        action: Action,
        message: Message,
        size: ButtonSize,
    ) -> NavButton {
        NavButton {
            id,
            icon,
            label_key,
            action: Some(action),
            message: Some(message),
            enabled: true,
            toggled: false,
            size,
        }
    }

    /// Turns the button off when `enabled` is false: it keeps its place but
    /// sends nothing.
    fn enabled_if(mut self, enabled: bool) -> NavButton {
        self.enabled = enabled;
        if !enabled {
            self.message = None;
        }
        self
    }

    fn toggled_if(mut self, toggled: bool) -> NavButton {
        self.toggled = toggled;
        self
    }
}

/// The twelve buttons of the bar, in reading order, for `nav` and the
/// autoplay state (rules 19 and 20 of the spec).
pub fn buttons(nav: &NavState, autoplay_running: bool) -> Vec<NavButton> {
    let mut all = primary_buttons(nav, autoplay_running);
    all.extend(secondary_buttons(nav));
    all
}

fn primary_buttons(nav: &NavState, autoplay_running: bool) -> Vec<NavButton> {
    use ButtonSize::Primary;
    let go = |n: Nav| game(GameMessage::Navigate(n));
    let (play_icon, play_label) = if autoplay_running {
        (Icon::Pause, "nav-pause")
    } else {
        (Icon::Play, "nav-play")
    };
    vec![
        NavButton::new(
            NAV_START,
            Icon::First,
            "nav-start",
            Action::GoStart,
            go(Nav::Start),
            Primary,
        )
        .enabled_if(!nav.at_start),
        NavButton::new(
            NAV_BACK,
            Icon::Back,
            "nav-back",
            Action::Back,
            go(Nav::Back(1)),
            Primary,
        )
        .enabled_if(!nav.at_start),
        NavButton::new(
            NAV_PLAY,
            play_icon,
            play_label,
            Action::ToggleAutoplay,
            game(GameMessage::ToggleAutoplay),
            Primary,
        )
        .toggled_if(autoplay_running),
        NavButton::new(
            NAV_FORWARD,
            Icon::Forward,
            "nav-forward",
            Action::Forward,
            go(Nav::Forward(1)),
            Primary,
        )
        .enabled_if(!nav.at_end),
        NavButton::new(
            NAV_END,
            Icon::Last,
            "nav-end",
            Action::GoEnd,
            go(Nav::GameEnd),
            Primary,
        )
        .enabled_if(!nav.at_end),
    ]
}

fn secondary_buttons(nav: &NavState) -> Vec<NavButton> {
    use ButtonSize::Secondary;
    let go = |n: Nav| game(GameMessage::Navigate(n));
    let trial = game(GameMessage::ToggleTrial { null_move: false });
    vec![
        NavButton::new(
            NAV_ADD_VARIATION,
            Icon::AddVariation,
            "nav-add-variation",
            Action::AddVariation,
            game(GameMessage::StartVariation),
            Secondary,
        )
        .enabled_if(nav.can_add_variation),
        NavButton::new(
            NAV_EXIT_VARIATION,
            Icon::ExitVariation,
            "nav-exit-variation",
            Action::ExitVariation,
            go(Nav::ExitVariation),
            Secondary,
        )
        .enabled_if(nav.in_variation),
        NavButton::new(
            NAV_ENTER_VARIATION,
            Icon::EnterVariation,
            "nav-enter-variation",
            Action::EnterVariation,
            game(GameMessage::OpenVariationChooser),
            Secondary,
        )
        .enabled_if(nav.variation_count > 0),
        NavButton::new(
            NAV_TRIAL,
            Icon::Trial,
            "nav-trial",
            Action::ToggleTrialMode,
            trial,
            Secondary,
        )
        .toggled_if(nav.trial),
        NavButton::new(
            NAV_UNDO,
            Icon::Undo,
            "nav-undo",
            Action::Undo,
            game(GameMessage::Undo),
            Secondary,
        )
        .enabled_if(nav.can_undo),
        NavButton::new(
            NAV_REDO,
            Icon::Redo,
            "nav-redo",
            Action::Redo,
            game(GameMessage::Redo),
            Secondary,
        )
        .enabled_if(nav.can_redo),
        NavButton::new(
            NAV_FLIP,
            Icon::Flip,
            "nav-flip",
            Action::FlipBoard,
            Message::Ui(UiMessage::FlipBoard),
            Secondary,
        ),
    ]
}

/// The tooltip of a button: its label and, when its action has a shortcut,
/// the first one as the map of the application has it now (it follows the
/// user's customisation), e.g. `Back (Left)` (D226).
pub fn tooltip_text(
    i18n: &I18n,
    button: &NavButton,
    shortcuts: &Shortcuts,
    platform: Platform,
) -> String {
    let label = i18n.tr(button.label_key);
    let binding = button
        .action
        .and_then(|action| shortcuts.bindings(action).first());
    match binding {
        Some(binding) => {
            let mut args = FluentArgs::new();
            args.set("label", label);
            args.set("shortcut", binding_label(binding, platform));
            i18n.tr_args("nav-tip", &args)
        }
        None => label,
    }
}

/// How much of the bar fits (D228).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarMode {
    /// 300 px or more: both rows.
    Full,
    /// From 220 to 300 px: the primary row and a "more" menu.
    Compact,
    /// Under 220 px: smaller primary buttons and the "more" menu.
    Narrow,
}

impl BarMode {
    /// The mode for a panel `width` in logical pixels.
    pub fn for_width(width: f32) -> BarMode {
        if width >= 300.0 {
            BarMode::Full
        } else if width >= 220.0 {
            BarMode::Compact
        } else {
            BarMode::Narrow
        }
    }
}

/// The size of one button.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonGeometry {
    /// Width in logical pixels; `None` shares the free width of the row.
    pub width: Option<f32>,
    /// Height in logical pixels.
    pub height: f32,
    /// Side of the icon.
    pub icon: f32,
}

/// Smallest clickable side (WCAG 2.2, 2.5.8).
pub const MIN_TARGET: f32 = 24.0;

impl BarMode {
    /// The geometry of a button; `wide` is the middle one (D227).
    pub fn geometry(self, size: ButtonSize, wide: bool) -> ButtonGeometry {
        match (self, size) {
            (BarMode::Full, ButtonSize::Primary) => ButtonGeometry {
                width: Some(if wide { 64.0 } else { 48.0 }),
                height: 40.0,
                icon: 20.0,
            },
            (BarMode::Compact, ButtonSize::Primary) => ButtonGeometry {
                width: None,
                height: 36.0,
                icon: 18.0,
            },
            (BarMode::Narrow, ButtonSize::Primary) => ButtonGeometry {
                width: None,
                height: 32.0,
                icon: 16.0,
            },
            // The "more" button of the narrowest panes gives its width to
            // the primary row, which must still reach 24 px per button.
            (BarMode::Narrow, ButtonSize::Secondary) => ButtonGeometry {
                width: Some(MIN_TARGET),
                height: 32.0,
                icon: 16.0,
            },
            (_, ButtonSize::Secondary) => ButtonGeometry {
                width: Some(32.0),
                height: 32.0,
                icon: 16.0,
            },
        }
    }
}

fn icon_tone(button: &NavButton, hero: bool) -> IconTone {
    if !button.enabled {
        IconTone::Disabled
    } else if button.toggled || hero {
        IconTone::OnAccent
    } else {
        IconTone::Text
    }
}

fn bar_button<'a>(
    button_data: &NavButton,
    tip: String,
    mode: BarMode,
    hero: bool,
    right: Option<Message>,
) -> Element<'a, Message> {
    let geometry = mode.geometry(button_data.size, hero);
    let look = match button_data.size {
        ButtonSize::Primary => ButtonLook::Primary,
        ButtonSize::Secondary => ButtonLook::Secondary,
    };
    let glyph = icon(
        button_data.icon,
        geometry.icon,
        icon_tone(button_data, hero),
    );
    let width = geometry.width.map_or(Length::Fill, Length::Fixed);
    let body = button(container(glyph).center(Length::Fill))
        .padding(0)
        .width(width)
        .height(Length::Fixed(geometry.height))
        .style(theme::command_button(look, button_data.toggled || hero))
        .on_press_maybe(button_data.message.clone());
    let with_tip = tooltip(
        body,
        container(text(tip).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Top,
    );
    // The right button of "Trial" is the threat analysis (spec M4-8).
    let content: Element<'a, Message> = match right {
        Some(message) => RightClick::new(with_tip, move |_, _| message.clone()).into(),
        None => with_tip.into(),
    };
    container(content).id(button_data.id).width(width).into()
}

/// What the right button of "Trial" sends: the threat of the first running
/// engine for which the application accepts it (D490). An engine whose
/// threat is not available (its analysis is locked, for instance) is
/// skipped, so the click is not lost while another one is free (D925).
fn threat_message(app: &AppState) -> Option<Message> {
    app.engine_runs()
        .iter()
        .filter(|r| r.state == EngineRunState::Running)
        .map(|r| r.id)
        .find(|run| app.mode_availability(*run).threat)
        .map(|run| Message::App(AppMessage::Mode(ModeMessage::ToggleThreat(run))))
}

fn menu_entry<'a>(
    button_data: &NavButton,
    label: String,
) -> Item<'a, Message, iced::Theme, iced::Renderer> {
    let tone = icon_tone(button_data, false);
    let content = row![icon(button_data.icon, 16.0, tone), text(label).size(13)]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([4, 8]));
    Item::new(
        button(content)
            .width(Length::Fill)
            .padding(0)
            .style(theme::menu_item)
            .on_press_maybe(button_data.message.clone()),
    )
}

/// The "more commands" button: opens a menu with the secondary commands.
fn more_menu<'a>(secondary: &[(NavButton, String)], mode: BarMode) -> Element<'a, Message> {
    let side = mode.geometry(ButtonSize::Secondary, false);
    let root =
        container(container(icon(Icon::More, side.icon, IconTone::Text)).center(Length::Fill))
            .id(NAV_MORE)
            .width(Length::Fixed(side.width.unwrap_or(MIN_TARGET)))
            .height(Length::Fixed(side.height));
    let entries = secondary
        .iter()
        .map(|(data, tip)| menu_entry(data, tip.clone()))
        .collect();
    let menu = Menu::new(entries).max_width(240.0).offset(2.0);
    crate::widgets::menu_guard::guard(MenuBar::new(vec![Item::with_menu(root, menu)]))
}

/// The command bar for the state of `app`: the five large buttons and, when
/// the panel is at least 300 px wide, the row of small ones; narrower
/// panels get a "more" menu instead (D228). It fills the width it is given.
pub fn command_bar<'a>(app: &AppState, i18n: &I18n, platform: Platform) -> Element<'a, Message> {
    let nav = NavState::of(&app.session);
    let threat = threat_message(app);
    let all: Vec<(NavButton, String)> = buttons(&nav, app.session.autoplay().running)
        .into_iter()
        .map(|data| {
            let tip = tooltip_text(i18n, &data, &app.shortcuts, platform);
            (data, tip)
        })
        .collect();
    responsive(move |size| {
        let mode = BarMode::for_width(size.width);
        let (primary, secondary): (Vec<_>, Vec<_>) = all
            .iter()
            .cloned()
            .partition(|(data, _)| data.size == ButtonSize::Primary);
        let spacing = if mode == BarMode::Full { 4.0 } else { 2.0 };
        let mut first = row![].spacing(spacing).align_y(iced::Alignment::Center);
        for (data, tip) in &primary {
            first = first.push(bar_button(
                data,
                tip.clone(),
                mode,
                data.id == NAV_PLAY,
                None,
            ));
        }
        let bar: Element<'_, Message> = match mode {
            BarMode::Full => {
                let mut left = row![].spacing(spacing).align_y(iced::Alignment::Center);
                let mut right = row![].spacing(spacing);
                for (data, tip) in &secondary {
                    let secondary_click = (data.id == NAV_TRIAL).then(|| threat.clone()).flatten();
                    let element = bar_button(data, tip.clone(), mode, false, secondary_click);
                    if data.id == NAV_FLIP {
                        right = right.push(element);
                    } else {
                        left = left.push(element);
                    }
                }
                iced::widget::column![
                    first.width(Length::Fill),
                    row![left, iced::widget::space::horizontal(), right]
                        .align_y(iced::Alignment::Center)
                ]
                .spacing(spacing)
                .into()
            }
            BarMode::Compact | BarMode::Narrow => {
                first = first.push(more_menu(&secondary, mode));
                first.width(Length::Fill).into()
            }
        };
        let side = if mode == BarMode::Narrow { 0.0 } else { 4.0 };
        container(bar).padding(Padding::from([4.0, side])).into()
    })
    .height(Length::Shrink)
    .into()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::update;

    use super::*;
    use crate::i18n::Language;

    fn sim(app: &AppState) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 120.0),
            command_bar(app, &I18n::new(Language::EnUs), Platform::Linux),
        )
    }

    fn field_sim(app: &AppState) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(300.0, 40.0),
            move_field(app, &I18n::new(Language::EnUs)),
        )
    }

    fn click(s: &mut Simulator<'static, Message>, id: &'static str) {
        s.click(iced::widget::Id::new(id)).expect("button exists");
    }

    fn play(app: &mut AppState, uci: &str, force: bool) {
        let mv = app.session.position().parse_uci(uci).expect("legal");
        update(
            app,
            AppMessage::Game(GameMessage::PlayMove {
                mv,
                options: mychess_app::PlayOptions {
                    force_variation: force,
                },
            }),
        );
    }

    #[test]
    fn edits_become_keystrokes() {
        assert_eq!(entry_edit("N", "Nf"), [GameMessage::MoveEntryChar('f')]);
        assert_eq!(
            entry_edit("Nf3", "N"),
            [
                GameMessage::MoveEntryBackspace,
                GameMessage::MoveEntryBackspace
            ]
        );
        assert_eq!(
            entry_edit("e4", "d4"),
            [
                GameMessage::MoveEntryClear,
                GameMessage::MoveEntryChar('d'),
                GameMessage::MoveEntryChar('4')
            ]
        );
        assert!(entry_edit("e", "e").is_empty());
    }

    #[test]
    fn at_the_start_of_an_empty_game_nothing_navigates() {
        let app = AppState::default();
        let mut s = sim(&app);
        for id in [
            NAV_START,
            NAV_BACK,
            NAV_FORWARD,
            NAV_END,
            NAV_EXIT_VARIATION,
            NAV_ENTER_VARIATION,
            NAV_ADD_VARIATION,
        ] {
            click(&mut s, id);
        }
        assert_eq!(s.into_messages().count(), 0, "all disabled");
    }

    #[test]
    fn enabled_buttons_follow_the_game_and_send_navigation() {
        let mut app = AppState::default();
        play(&mut app, "e2e4", false);
        let mut s = sim(&app);
        click(&mut s, NAV_BACK);
        click(&mut s, NAV_START);
        click(&mut s, NAV_ADD_VARIATION);
        click(&mut s, NAV_TRIAL);
        click(&mut s, NAV_PLAY);
        click(&mut s, NAV_FLIP);
        click(&mut s, NAV_FORWARD); // disabled at the end
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [
                Message::App(AppMessage::Game(GameMessage::Navigate(Nav::Back(1)))),
                Message::App(AppMessage::Game(GameMessage::Navigate(Nav::Start))),
                Message::App(AppMessage::Game(GameMessage::StartVariation)),
                Message::App(AppMessage::Game(GameMessage::ToggleTrial {
                    null_move: false
                })),
                Message::App(AppMessage::Game(GameMessage::ToggleAutoplay)),
                Message::Ui(UiMessage::FlipBoard),
            ]
        ));
    }

    #[test]
    fn exit_variation_is_on_only_inside_one() {
        let mut app = AppState::default();
        play(&mut app, "e2e4", false);
        update(
            &mut app,
            AppMessage::Game(GameMessage::Navigate(Nav::Start)),
        );
        play(&mut app, "d2d4", true);
        let mut s = sim(&app);
        click(&mut s, NAV_EXIT_VARIATION);
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Game(GameMessage::Navigate(
                Nav::ExitVariation
            )))]
        ));
    }

    #[test]
    fn the_field_shows_the_buffer_and_forwards_typing() {
        let mut app = AppState::default();
        update(&mut app, AppMessage::Game(GameMessage::MoveEntryChar('N')));
        let mut s = field_sim(&app);
        s.click(iced::widget::Id::new(MOVE_FIELD_ID))
            .expect("field");
        s.typewrite("f");
        s.tap_key(iced::keyboard::key::Named::Enter);
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [
                Message::Dialog(DialogMessage::MoveEntryKeys(keys)),
                Message::App(AppMessage::Game(GameMessage::MoveEntryCommit)),
            ] if keys == &[GameMessage::MoveEntryChar('f')]
        ));
    }

    // --- The icon command bar ---------------------------------------

    fn state() -> NavState {
        NavState {
            at_start: false,
            at_end: false,
            in_variation: false,
            can_add_variation: false,
            variation_count: 0,
            trial: false,
            can_undo: false,
            can_redo: false,
        }
    }

    fn by_id<'a>(list: &'a [NavButton], id: &str) -> &'a NavButton {
        list.iter().find(|b| b.id == id).expect("button")
    }

    #[test]
    fn the_twelve_buttons_come_in_reading_order_with_unique_ids() {
        let list = buttons(&state(), false);
        let ids: Vec<_> = list.iter().map(|b| b.id).collect();
        assert_eq!(
            ids,
            [
                "nav-start",
                "nav-back",
                "nav-play",
                "nav-forward",
                "nav-end",
                "nav-add-variation",
                "nav-exit-variation",
                "nav-enter-variation",
                "nav-trial",
                "nav-undo",
                "nav-redo",
                "nav-flip",
            ]
        );
        let primary = list
            .iter()
            .filter(|b| b.size == ButtonSize::Primary)
            .count();
        assert_eq!(primary, 5);
    }

    #[test]
    fn a_button_is_enabled_exactly_when_the_nav_state_allows_it() {
        let enabled = |nav: &NavState, running: bool| -> Vec<(&'static str, bool, bool)> {
            buttons(nav, running)
                .iter()
                .map(|b| (b.id, b.enabled, b.toggled))
                .collect()
        };
        // Middle of a game, nothing special: the movement buttons are on.
        let mid = enabled(&state(), false);
        for (id, on, toggled) in &mid {
            let expected = matches!(
                *id,
                "nav-start"
                    | "nav-back"
                    | "nav-play"
                    | "nav-forward"
                    | "nav-end"
                    | "nav-trial"
                    | "nav-flip"
            );
            assert_eq!(*on, expected, "{id}");
            assert!(!toggled, "{id}");
        }
        // Start of an empty game.
        let empty = NavState {
            at_start: true,
            at_end: true,
            ..state()
        };
        for (id, on, _) in enabled(&empty, false) {
            let expected = matches!(id, "nav-play" | "nav-trial" | "nav-flip");
            assert_eq!(on, expected, "{id}");
        }
        // Every flag turns on its own button.
        let all = NavState {
            at_start: false,
            at_end: false,
            in_variation: true,
            can_add_variation: true,
            variation_count: 2,
            trial: true,
            can_undo: true,
            can_redo: true,
        };
        let list = buttons(&all, true);
        assert!(list.iter().all(|b| b.enabled));
        assert!(by_id(&list, "nav-trial").toggled);
        assert!(by_id(&list, "nav-play").toggled);
        assert_eq!(by_id(&list, "nav-play").icon, Icon::Pause);
        assert_eq!(by_id(&list, "nav-play").label_key, "nav-pause");
        let idle = buttons(&state(), false);
        assert_eq!(by_id(&idle, "nav-play").icon, Icon::Play);
        assert_eq!(by_id(&idle, "nav-play").label_key, "nav-play");
        for (flag, id) in [
            (
                NavState {
                    in_variation: true,
                    ..state()
                },
                "nav-exit-variation",
            ),
            (
                NavState {
                    can_add_variation: true,
                    ..state()
                },
                "nav-add-variation",
            ),
            (
                NavState {
                    variation_count: 1,
                    ..state()
                },
                "nav-enter-variation",
            ),
            (
                NavState {
                    can_undo: true,
                    ..state()
                },
                "nav-undo",
            ),
            (
                NavState {
                    can_redo: true,
                    ..state()
                },
                "nav-redo",
            ),
        ] {
            let list = buttons(&flag, false);
            assert!(by_id(&list, id).enabled, "{id}");
            assert_eq!(list.iter().filter(|b| b.enabled).count(), 8, "{id}");
        }
    }

    #[test]
    fn a_disabled_button_carries_no_message() {
        for b in buttons(&state(), false) {
            assert_eq!(b.message.is_some(), b.enabled, "{}", b.id);
        }
    }

    #[test]
    fn every_label_exists_in_both_languages() {
        for language in [Language::EnUs, Language::PtBr] {
            let i18n = I18n::new(language);
            for running in [false, true] {
                for b in buttons(&state(), running) {
                    assert!(i18n.has(b.label_key), "{language:?} {}", b.label_key);
                }
            }
            assert!(i18n.has("nav-more") && i18n.has("nav-tip"));
        }
    }

    #[test]
    fn tooltips_show_the_first_shortcut_of_the_current_map() {
        let i18n = I18n::new(Language::EnUs);
        let app = AppState::default();
        let list = buttons(&state(), false);
        let back = by_id(&list, "nav-back");
        assert_eq!(
            tooltip_text(&i18n, back, &app.shortcuts, Platform::Linux),
            "Back (Left)"
        );
        let pt = I18n::new(Language::PtBr);
        assert_eq!(
            tooltip_text(&pt, back, &app.shortcuts, Platform::Linux),
            "Voltar (Left)"
        );
        let start = by_id(&list, "nav-start");
        let text = tooltip_text(&i18n, start, &app.shortcuts, Platform::MacOs);
        assert!(text.starts_with("Start ("), "{text}");
    }

    #[test]
    fn a_button_whose_action_has_no_shortcut_shows_only_the_label() {
        let i18n = I18n::new(Language::EnUs);
        let list = buttons(&state(), false);
        let mut button = by_id(&list, "nav-back").clone();
        button.action = None;
        let app = AppState::default();
        assert_eq!(
            tooltip_text(&i18n, &button, &app.shortcuts, Platform::Linux),
            "Back"
        );
    }

    #[test]
    fn a_customised_shortcut_reaches_the_tooltip() {
        let i18n = I18n::new(Language::EnUs);
        let mut app = AppState::default();
        let custom =
            std::collections::BTreeMap::from([("Back".to_owned(), "Command+B".to_owned())]);
        let (map, _) = app.shortcuts.clone().with_overrides(&custom);
        app.shortcuts = map;
        let list = buttons(&state(), false);
        assert_eq!(
            tooltip_text(
                &i18n,
                by_id(&list, "nav-back"),
                &app.shortcuts,
                Platform::Linux
            ),
            "Back (Ctrl+B)"
        );
    }

    #[test]
    fn the_three_widths_pick_the_modes_of_the_spec() {
        assert_eq!(BarMode::for_width(1000.0), BarMode::Full);
        assert_eq!(BarMode::for_width(300.0), BarMode::Full);
        assert_eq!(BarMode::for_width(299.9), BarMode::Compact);
        assert_eq!(BarMode::for_width(220.0), BarMode::Compact);
        assert_eq!(BarMode::for_width(219.9), BarMode::Narrow);
        assert_eq!(BarMode::for_width(0.0), BarMode::Narrow);
    }

    #[test]
    fn button_sizes_follow_the_spec_and_stay_clickable() {
        let full = BarMode::Full;
        assert_eq!(
            full.geometry(ButtonSize::Primary, false),
            ButtonGeometry {
                width: Some(48.0),
                height: 40.0,
                icon: 20.0
            }
        );
        assert_eq!(full.geometry(ButtonSize::Primary, true).width, Some(64.0));
        assert_eq!(
            full.geometry(ButtonSize::Secondary, false),
            ButtonGeometry {
                width: Some(32.0),
                height: 32.0,
                icon: 16.0
            }
        );
        assert_eq!(
            BarMode::Compact.geometry(ButtonSize::Primary, false).height,
            36.0
        );
        assert_eq!(
            BarMode::Narrow.geometry(ButtonSize::Primary, false).height,
            32.0
        );
        for mode in [BarMode::Full, BarMode::Compact, BarMode::Narrow] {
            for size in [ButtonSize::Primary, ButtonSize::Secondary] {
                let g = mode.geometry(size, false);
                assert!(g.height >= MIN_TARGET, "{mode:?} {size:?}");
                assert!(g.width.is_none_or(|w| w >= MIN_TARGET), "{mode:?} {size:?}");
            }
        }
        // In the narrowest pane (160 px, no side padding) every shared
        // button still gets 24 px: 160 - "more" - 5 gaps of 2 px.
        let more = BarMode::Narrow.geometry(ButtonSize::Secondary, false);
        let share = (160.0_f32 - more.width.unwrap_or(0.0) - 10.0) / 5.0;
        assert!(share >= MIN_TARGET, "{share}");
    }

    fn bar_sim(app: &AppState, width: f32) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(width, 140.0),
            command_bar(app, &I18n::new(Language::EnUs), Platform::Linux),
        )
    }

    fn id(name: &'static str) -> iced::widget::Id {
        iced::widget::Id::new(name)
    }

    #[test]
    fn the_wide_bar_has_all_twelve_buttons_and_sends_the_messages_of_today() {
        let mut app = AppState::default();
        play(&mut app, "e2e4", false);
        let mut s = bar_sim(&app, 360.0);
        for name in [
            NAV_START,
            NAV_BACK,
            NAV_PLAY,
            NAV_FORWARD,
            NAV_END,
            NAV_ADD_VARIATION,
            NAV_EXIT_VARIATION,
            NAV_ENTER_VARIATION,
            NAV_TRIAL,
            NAV_UNDO,
            NAV_REDO,
            NAV_FLIP,
        ] {
            assert!(s.find(id(name)).is_ok(), "{name} is on the bar");
        }
        assert!(s.find(id(NAV_MORE)).is_err(), "no menu button when wide");
        for name in [
            NAV_BACK,
            NAV_START,
            NAV_ADD_VARIATION,
            NAV_TRIAL,
            NAV_PLAY,
            NAV_UNDO,
            NAV_FLIP,
        ] {
            s.click(id(name)).expect("button");
        }
        // Off at the end of the game: forward and end; no variation to exit.
        for name in [
            NAV_FORWARD,
            NAV_END,
            NAV_EXIT_VARIATION,
            NAV_ENTER_VARIATION,
            NAV_REDO,
        ] {
            s.click(id(name)).expect("button");
        }
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(
            matches!(
                sent.as_slice(),
                [
                    Message::App(AppMessage::Game(GameMessage::Navigate(Nav::Back(1)))),
                    Message::App(AppMessage::Game(GameMessage::Navigate(Nav::Start))),
                    Message::App(AppMessage::Game(GameMessage::StartVariation)),
                    Message::App(AppMessage::Game(GameMessage::ToggleTrial {
                        null_move: false
                    })),
                    Message::App(AppMessage::Game(GameMessage::ToggleAutoplay)),
                    Message::App(AppMessage::Game(GameMessage::Undo)),
                    Message::Ui(UiMessage::FlipBoard),
                ]
            ),
            "{sent:?}"
        );
    }

    #[test]
    fn nothing_is_sent_by_the_disabled_buttons_of_an_empty_game() {
        let app = AppState::default();
        let mut s = bar_sim(&app, 360.0);
        for name in [
            NAV_START,
            NAV_BACK,
            NAV_FORWARD,
            NAV_END,
            NAV_ADD_VARIATION,
            NAV_EXIT_VARIATION,
            NAV_ENTER_VARIATION,
            NAV_UNDO,
            NAV_REDO,
        ] {
            s.click(id(name)).expect("button");
        }
        assert_eq!(s.into_messages().count(), 0);
    }

    #[test]
    fn the_exit_button_works_inside_a_variation() {
        let mut app = AppState::default();
        play(&mut app, "e2e4", false);
        update(
            &mut app,
            AppMessage::Game(GameMessage::Navigate(Nav::Start)),
        );
        play(&mut app, "d2d4", true);
        let mut s = bar_sim(&app, 360.0);
        s.click(id(NAV_EXIT_VARIATION)).expect("button");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Game(GameMessage::Navigate(
                Nav::ExitVariation
            )))]
        ));
    }

    #[test]
    fn narrower_panels_keep_the_primary_row_and_add_a_more_menu() {
        let app = AppState::default();
        for width in [260.0, 180.0] {
            let mut s = bar_sim(&app, width);
            for name in [
                NAV_START,
                NAV_BACK,
                NAV_PLAY,
                NAV_FORWARD,
                NAV_END,
                NAV_MORE,
            ] {
                assert!(s.find(id(name)).is_ok(), "{name} at {width}");
            }
            for name in [NAV_ADD_VARIATION, NAV_UNDO, NAV_FLIP] {
                assert!(s.find(id(name)).is_err(), "{name} hides at {width}");
            }
        }
    }

    #[test]
    fn the_right_click_of_trial_needs_a_running_engine() {
        // Without an engine there is nothing to run the threat on (spec M4-8).
        assert!(threat_message(&AppState::default()).is_none());
    }
}
