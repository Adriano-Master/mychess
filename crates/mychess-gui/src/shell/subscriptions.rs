//! Event sources of the shell: frames (only while animating), the system
//! theme, the keyboard, the window and the flush of the preferences.

use std::time::Duration;

use iced::futures::{Stream, stream};
use iced::{Subscription, event, keyboard, mouse, time, window};
use mychess_app::Message as AppMessage;
use mychess_app::engines::{EngineEvent, EngineHub, EngineMessage, RunId};
use mychess_app::online::OnlineHub;

use super::{Message, Shell, UiMessage};
use crate::keymap;
use crate::windows::dialog_ui::DialogMessage;
use crate::windows::game_list::GameListMessage;
use crate::windows::notation::{self, NotationMessage};

/// How often the timers that expire things are checked: a remark of the
/// status bar (8 s) and a half-typed NAG sequence (1.5 s).
const EXPIRY_CHECK: Duration = Duration::from_millis(500);
/// A different period for the NAG timer: two `time::every` with the same
/// period would be the same subscription.
const NAG_CHECK: Duration = Duration::from_millis(250);
/// How often the progress of a database being opened is redrawn: the
/// application keeps it in an atomic, so nothing says when it changes.
const OPEN_PROGRESS_CHECK: Duration = Duration::from_millis(150);
/// How often the clocks of a game against an engine tick (D1251).
pub(super) const PLAY_TICK: Duration = Duration::from_millis(100);
/// How often the clocks of an online game tick (D1475). A different period
/// from `PLAY_TICK`: two `time::every` with the same one would be the same
/// subscription.
const ONLINE_TICK: Duration = Duration::from_millis(125);
/// How often an open engine log reads the new entries (spec M4-4, D447).
const LOG_CHECK: Duration = Duration::from_millis(250);

/// The character a key press typed, when it is one printable character.
fn typed_char(text: Option<&str>) -> Option<char> {
    let mut chars = text?.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    (!c.is_control()).then_some(c)
}

/// How often changed preferences are written while there are unsaved ones:
/// a change is on disk at most this long after it was made.
pub(super) const FLUSH_INTERVAL: Duration = Duration::from_secs(2);

fn on_event(event: iced::Event, status: event::Status, _window: window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            repeat,
            ..
        }) => {
            // Holding Enter must not answer the question that the first
            // press opened (the confirmation of a destructive operation).
            if repeat && matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter)) {
                return None;
            }
            let input = keymap::key_input(&key, modifiers)?;
            // A widget that captured the key used it for text.
            Some(Message::Dialog(DialogMessage::Key {
                input,
                text_focused: status == event::Status::Captured,
                typed: typed_char(text.as_deref()),
            }))
        }
        // The keys the selection of the list looks at (Ctrl/Cmd, Shift).
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Message::Games(GameListMessage::Modifiers {
                command: modifiers.command(),
                shift: modifiers.shift(),
            }))
        }
        // Any click: the comment editor may have lost the focus. Judged on
        // the release, after the button that was clicked has acted.
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::Notation(NotationMessage::PointerReleased))
        }
        // A press no widget took, the window losing the focus or the pointer
        // leaving it: a release may never arrive, so the drag of the game
        // list ends (a press on a tab or row is captured by its widget and
        // does not get here, D594).
        iced::Event::Mouse(mouse::Event::ButtonPressed(_)) if status == event::Status::Ignored => {
            Some(Message::Games(GameListMessage::DragEnd))
        }
        iced::Event::Mouse(mouse::Event::CursorLeft)
        | iced::Event::Window(window::Event::Unfocused) => {
            Some(Message::Games(GameListMessage::DragEnd))
        }
        _ => None,
    }
}

/// The message that carries an event of a running engine.
pub(super) fn engine_message(run: RunId, event: EngineEvent) -> Message {
    Message::App(AppMessage::Engine(EngineMessage::Event {
        run,
        event: Box::new(event),
    }))
}

/// The events of every engine as messages (spec M4-3, D435). The receiver
/// leaves the hub the first time the stream is built; a stream that cannot
/// get it (already taken, or the hub is gone) ends at once, and the
/// subscription stays quiet.
pub(super) fn engine_stream(hub: &EngineHub) -> impl Stream<Item = Message> + use<> {
    stream::unfold(hub.events(), |events| async move {
        let mut events = events?;
        let (run, event) = events.next().await?;
        Some((engine_message(run, event), Some(events)))
    })
}

/// The subscription over the events of the engines. The hub is the
/// identity of the subscription (compared by pointer), so it runs for as
/// long as the application does.
fn engine_events(shell: &Shell) -> Subscription<Message> {
    Subscription::run_with(shell.app.engine_hub.clone(), engine_stream)
}

/// The events of the streams of Lichess as messages (spec M8-3c, D1310).
/// Like the engine events, the receiver leaves the hub once.
pub(super) fn online_stream(hub: &OnlineHub) -> impl Stream<Item = Message> + use<> {
    stream::unfold(hub.events(), |events| async move {
        let mut events = events?;
        let message = events.next().await?;
        Some((Message::App(message), Some(events)))
    })
}

/// The subscription over the events of the online play; it runs for as
/// long as the application does.
fn online_events(shell: &Shell) -> Subscription<Message> {
    Subscription::run_with(shell.app.online.hub.clone(), online_stream)
}

/// The events of the FICS connection as messages; the receiver leaves the
/// hub once, like the others.
pub(super) fn fics_stream(hub: &mychess_app::fics::FicsHub) -> impl Stream<Item = Message> + use<> {
    stream::unfold(hub.events(), |events| async move {
        let mut events = events?;
        let message = events.next().await?;
        Some((
            Message::App(mychess_app::Message::Fics(message)),
            Some(events),
        ))
    })
}

fn fics_events(shell: &Shell) -> Subscription<Message> {
    Subscription::run_with(shell.app.fics.hub.clone(), fics_stream)
}

/// The events of the electronic board as messages (spec M8-10c); the
/// receiver leaves the hub once, like the others.
pub(super) fn eboard_stream(
    hub: &mychess_app::eboard::EboardHub,
) -> impl Stream<Item = Message> + use<> {
    stream::unfold(hub.events(), |events| async move {
        let mut events = events?;
        let message = events.next().await?;
        Some((Message::App(message), Some(events)))
    })
}

fn eboard_events(shell: &Shell) -> Subscription<Message> {
    Subscription::run_with(shell.app.eboard().hub().clone(), eboard_stream)
}

/// All subscriptions for the current state.
#[allow(clippy::too_many_lines)] // one flat list of the subscriptions
pub(super) fn subscription(shell: &Shell) -> Subscription<Message> {
    let mut all = vec![
        event::listen_with(on_event),
        iced::system::theme_changes().map(|mode| Message::Ui(UiMessage::SystemTheme(mode))),
        window::close_requests().map(|_| Message::Ui(UiMessage::CloseRequested)),
        window::resize_events().map(|(_, size)| Message::Ui(UiMessage::WindowResized(size))),
    ];
    if shell.ui.is_animating() {
        all.push(window::frames().map(|now| Message::Ui(UiMessage::AnimationFrame(now))));
    }
    if shell.app.prefs_dirty {
        all.push(time::every(FLUSH_INTERVAL).map(|_| Message::Ui(UiMessage::FlushTick)));
    }
    if shell.has_temporary_notice() {
        all.push(
            time::every(EXPIRY_CHECK).map(|now| Message::Dialog(DialogMessage::StatusTick(now))),
        );
    }
    if shell.app.base_opening().is_some()
        || shell.app.list_job().is_some()
        || shell.app.tree_cache_busy()
        // The progress of the export of a book branch (M5-6d, D2164).
        || shell.app.book().export_progress().is_some()
        // The progress of a job of the masks (D1876 5).
        || shell.app.mask_busy().is_some()
        || shell
            .app
            .browser
            .as_ref()
            .is_some_and(mychess_app::BrowserState::is_loading)
    {
        all.push(
            time::every(OPEN_PROGRESS_CHECK).map(|_| Message::Ui(UiMessage::OpenProgressTick)),
        );
    }
    // The clocks of a game against an engine (spec M8-1b, D1251).
    if shell.app.play_wants_ticks() {
        all.push(
            time::every(PLAY_TICK)
                .map(|at| Message::App(AppMessage::Play(mychess_app::PlayMessage::Tick(at)))),
        );
    }
    // The clock and the watchdog of the tactics training (spec M8-4, D1925).
    if shell.app.tactics_wants_ticks() {
        all.push(time::every(Duration::from_secs(1)).map(|at| {
            Message::App(AppMessage::Tactics(
                mychess_app::tactics::TacticsMessage::Tick(at),
            ))
        }));
    }
    // The watchdog of the opening trainer while it reads the repertoire
    // (spec M8-5, D2231).
    if shell.app.opening_wants_ticks() {
        all.push(time::every(Duration::from_secs(1)).map(|at| {
            Message::App(AppMessage::OpeningTrainer(
                mychess_app::opening_trainer::OpeningMessage::Tick(at),
            ))
        }));
    }
    // The progress of an advanced search (spec M6-1, D3380): the front-end
    // copies it to the state every 100 ms while one runs.
    if shell.app.advanced_search().is_running() {
        all.push(time::every(Duration::from_millis(100)).map(|_| {
            Message::App(AppMessage::AdvancedSearch(
                mychess_app::AdvancedSearchMessage::Progress,
            ))
        }));
    }
    // The progress bars of the windows of the players, tournaments and
    // spelling (spec M6-3c): the views read the progress; a redraw is enough.
    if crate::windows::player_tools::wants_ticks(&shell.app) {
        all.push(
            time::every(Duration::from_millis(150)).map(|_| {
                Message::PlayerTools(crate::windows::player_tools::PlayerToolsMessage::Tick)
            }),
        );
    }
    // The progress of a search for patterns (spec M6-2, slice c): the same
    // tick of 100 ms.
    if shell.app.pattern_search().is_running() {
        all.push(time::every(Duration::from_millis(100)).map(|_| {
            Message::App(AppMessage::PatternSearch(
                mychess_app::PatternSearchMessage::Progress,
            ))
        }));
    }
    // The clock and the watchdog of the calculation exercise (spec M8-6,
    // D2306, D2310).
    if shell.app.calvar_wants_ticks() {
        all.push(time::every(Duration::from_secs(1)).map(|at| {
            Message::App(AppMessage::Calvar(
                mychess_app::calvar::CalvarMessage::Tick(at),
            ))
        }));
    }
    // The clocks of the live tab of the tournament (D1366): no timer unless
    // that tab is on screen.
    if shell.ui.dialogs.tournament.wants_ticks(
        &shell.app,
        shell
            .ui
            .is_visible(crate::windows::layout::PaneKind::Tournament),
    ) {
        all.push(time::every(PLAY_TICK).map(|at| {
            Message::Tournament(crate::windows::tournament_ui::TournamentUiMessage::Tick(at))
        }));
    }
    // The clocks of an online game: no timer unless one is on (D1475).
    if crate::windows::online_ui::wants_ticks(&shell.app) {
        all.push(
            time::every(ONLINE_TICK)
                .map(|at| Message::Online(crate::windows::online_ui::OnlineUiMessage::Tick(at))),
        );
    }
    // The clocks of a FICS game (D2613): no timer unless one is on.
    if crate::windows::fics_dialog::wants_ticks(&shell.app) {
        all.push(
            time::every(ONLINE_TICK)
                .map(|at| Message::Fics(crate::windows::fics_dialog::FicsUiMessage::Tick(at))),
        );
    }
    // The limit of the save before quitting (D1796).
    if matches!(
        shell.ui.dialogs.book.quit,
        Some(
            crate::windows::book_panel::BookQuit::Saving { .. }
                | crate::windows::book_panel::BookQuit::Draining { .. }
        )
    ) {
        all.push(time::every(Duration::from_secs(1)).map(|_| Message::Ui(UiMessage::BookQuitTick)));
    }
    if shell.app.nag_sequence_deadline().is_some() {
        all.push(time::every(NAG_CHECK).map(|now| Message::Dialog(DialogMessage::NagTick(now))));
    }
    // Only while a log is open and refreshes itself.
    if shell.analysis_log_ticking() {
        all.push(time::every(LOG_CHECK).map(|_| {
            Message::Ui(UiMessage::Analysis(
                crate::windows::analysis::AnalysisUiMessage::LogTickAll,
            ))
        }));
    }
    // Must stay unconditional: the hub's receiver is taken once (D745).
    all.push(engine_events(shell));
    all.push(online_events(shell));
    all.push(fics_events(shell));
    all.push(eboard_events(shell));
    all.extend(notation::subscriptions(shell));
    Subscription::batch(all)
}

#[cfg(test)]
mod tests {
    use iced::futures::StreamExt;
    use mychess_app::engines::EngineEvent;

    use super::*;

    fn enter_event(repeat: bool) -> iced::Event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Enter),
            modified_key: keyboard::Key::Named(keyboard::key::Named::Enter),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat,
        })
    }

    #[test]
    fn a_repeated_enter_is_dropped_but_the_first_press_goes_through() {
        let id = window::Id::unique();
        assert!(on_event(enter_event(false), event::Status::Ignored, id).is_some());
        assert!(on_event(enter_event(true), event::Status::Ignored, id).is_none());
    }

    #[test]
    fn an_event_becomes_the_message_of_its_run() {
        let message = engine_message(
            RunId(7),
            EngineEvent::Warning(mychess_app::engines::EngineWarning::IllegalBestMove(
                "x".to_owned(),
            )),
        );
        assert!(matches!(
            message,
            Message::App(AppMessage::Engine(EngineMessage::Event { run, .. })) if run == RunId(7)
        ));
    }

    #[test]
    fn the_online_stream_takes_the_receiver_once_and_later_streams_end_at_once() {
        let hub = OnlineHub::new();
        let first = online_stream(&hub);
        let mut second = Box::pin(online_stream(&hub));
        let ended = iced::futures::executor::block_on(second.next());
        assert!(ended.is_none(), "the receiver is already taken");
        drop(first);
    }

    #[test]
    fn the_stream_takes_the_receiver_once_and_later_streams_end_at_once() {
        let hub = EngineHub::new();
        let first = engine_stream(&hub);
        let mut second = Box::pin(engine_stream(&hub));
        let ended = iced::futures::executor::block_on(second.next());
        assert!(ended.is_none(), "the receiver is already taken");
        drop(first);
    }
}
