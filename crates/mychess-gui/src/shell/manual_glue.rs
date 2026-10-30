//! The part of the shell that serves the manual window (spec M9-5, slice
//! c): it opens the window with the index of the language, parses the
//! chapter once, scrolls to the place the reader moved to and turns the
//! keys of the window into messages.

use iced::Task;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{operation, selector};
use mychess_app::manual::{HelpContext, ManualMessage, help_target};
use mychess_app::{Action, KeyInput, Message as AppMessage};

use super::{Message, Shell};
use crate::widgets::modal;
use crate::windows::manual::{
    MANUAL_FIND_ID, MANUAL_SCROLL_ID, ManualUi, ManualUiMessage, Rendered, chapter_texts,
    heading_id, shared_index,
};

/// How far an arrow key scrolls the text, in pixels.
const LINE_STEP: f32 = 48.0;

fn manual(message: ManualMessage) -> AppMessage {
    AppMessage::Manual(message)
}

impl Shell {
    /// Whether the manual covers the main window.
    pub(super) fn manual_layer_open(&self) -> bool {
        ManualUi::layer_open(&self.app)
    }

    /// The context `F1` opens the manual for: the window that is open, or
    /// the board (D4062). Windows that take the keyboard for themselves
    /// never get here.
    ///
    /// The windows that cover the program come first, then the tool
    /// windows, then the visible panels other than the ones of the board
    /// (the board, the information, the notation, the comment and the game
    /// list belong to the default chapters).
    fn help_context(&self) -> HelpContext {
        use crate::windows::layout::PaneKind;
        if self.maint_layer_open()
            || self.oprep_layer_open()
            || self.preport_layer_open()
            || self.epd_layer_open()
            || crate::windows::merge_dialog::is_open(&self.app)
        {
            return HelpContext::Reports;
        }
        if crate::windows::player_tools::ToolWindow::ALL
            .into_iter()
            .any(|window| window.is_open(&self.app))
        {
            return HelpContext::Players;
        }
        let shown = |kind| self.ui.is_visible(kind);
        if shown(PaneKind::Tree) || shown(PaneKind::Book) {
            HelpContext::Openings
        } else if shown(PaneKind::Analysis)
            || shown(PaneKind::Report)
            || shown(PaneKind::Tablebase)
            || shown(PaneKind::Graph)
        {
            HelpContext::Engines
        } else if shown(PaneKind::Tournament) {
            HelpContext::Play
        } else if shown(PaneKind::Crosstable)
            || shown(PaneKind::FilterStats)
            || shown(PaneKind::RatingGraph)
            || shown(PaneKind::PieceTracker)
        {
            HelpContext::Players
        } else if shown(PaneKind::Epd) {
            HelpContext::Reports
        } else if self.app.browser.is_some() {
            HelpContext::Databases
        } else {
            HelpContext::Board
        }
    }

    /// Opens the manual at a chapter, with the index of the language of the
    /// interface.
    fn open_manual_at(&mut self, chapter: usize, anchor: Option<String>) -> Task<Message> {
        let index = shared_index(self.i18n.language());
        self.dispatch(manual(ManualMessage::Open {
            index,
            chapter,
            anchor,
        }))
    }

    /// `F1`: opens the manual for the context, or closes it when it is
    /// already open.
    pub(super) fn toggle_manual(&mut self) -> Task<Message> {
        if self.manual_layer_open() {
            return self.update_manual(ManualUiMessage::Plain(ManualMessage::Close));
        }
        let (chapter, anchor) = help_target(self.help_context());
        self.open_manual_at(chapter, anchor.map(str::to_owned))
    }

    /// Brings the window up to date with the application: the index follows
    /// the language, the chapter is parsed when it changed, and a move of
    /// the reader asks for the scroll. A closed window forgets its chapter.
    pub(super) fn sync_manual(&mut self) -> Task<Message> {
        if !self.app.manual().is_open() {
            if self.ui.dialogs.manual.was_open {
                self.ui.dialogs.manual.reset();
                self.ui.dialogs.manual.was_open = false;
                // The search field must not keep the keys.
                return modal::unfocus_all();
            }
            return Task::none();
        }
        self.ui.dialogs.manual.was_open = true;
        let language = self.i18n.language();
        let mut tasks = Vec::new();
        let wanted = shared_index(language);
        if self.app.manual().shared_index() != Some(&wanted) {
            tasks.push(self.dispatch(manual(ManualMessage::Reindex(wanted))));
        }
        let chapter = self.app.manual().chapter();
        let stale = self
            .ui
            .dialogs
            .manual
            .rendered()
            .is_none_or(|r| r.chapter != chapter || r.language != language);
        if stale && let Some((texts, _)) = chapter_texts(language) {
            let text = texts.get(chapter).copied().unwrap_or_default();
            if let Some(index) = self.app.manual().index() {
                self.ui.dialogs.manual.rendered =
                    Some(Rendered::build(text, index, chapter, language));
            }
        }
        let serial = self.app.manual().nav_serial();
        if serial != self.ui.dialogs.manual.served {
            self.ui.dialogs.manual.served = serial;
            // The view of the new chapter is built before the scroll runs:
            // the operation finds the widgets of the next frame.
            tasks.push(Task::done(Message::ManualUi(ManualUiMessage::ScrollNow)));
        }
        Task::batch(tasks)
    }

    fn scroll_by(delta: f32) -> Task<Message> {
        operation::scroll_by(MANUAL_SCROLL_ID, AbsoluteOffset { x: 0.0, y: delta })
    }

    fn scroll_to(y: f32) -> Task<Message> {
        operation::scroll_to(MANUAL_SCROLL_ID, AbsoluteOffset { x: 0.0, y })
    }

    /// Asks whether the search field has the focus.
    fn probe_manual_focus() -> Task<Message> {
        operation::is_focused(MANUAL_FIND_ID)
            .map(|on| Message::ManualUi(ManualUiMessage::SearchFocus(on)))
    }

    /// A key while the window is open: Esc, Ctrl/Cmd+F, `F1`, Alt+arrows,
    /// Backspace and the keys that scroll act; the game behind is blocked.
    pub(super) fn manual_key(&mut self, input: &KeyInput, text_focused: bool) -> Task<Message> {
        use mychess_app::{Key, Modifiers, NamedKey};
        let text_focused = text_focused || self.ui.dialogs.manual.search_focused;
        let page = (self.ui.dialogs.manual.viewport * 0.9).max(LINE_STEP);
        let task = match (input.key, input.modifiers) {
            (Key::Named(NamedKey::Escape), Modifiers::NONE) => {
                let state = self.app.manual();
                if state.pending_external().is_some() {
                    self.dispatch(manual(ManualMessage::ConfirmExternal(false)))
                } else if !state.query().is_empty() {
                    self.dispatch(manual(ManualMessage::Query(String::new())))
                } else {
                    self.update_manual(ManualUiMessage::Plain(ManualMessage::Close))
                }
            }
            (Key::Char('f'), m) if m.command && !m.shift && !m.alt => {
                self.update_manual(ManualUiMessage::FocusFind)
            }
            (Key::Named(NamedKey::Left), m) if m.alt && !m.command && !m.shift => {
                self.update_manual(ManualUiMessage::Nav(ManualMessage::Back))
            }
            (Key::Named(NamedKey::Right), m) if m.alt && !m.command && !m.shift => {
                self.update_manual(ManualUiMessage::Nav(ManualMessage::Forward))
            }
            (Key::Named(NamedKey::Backspace), Modifiers::NONE) if !text_focused => {
                self.update_manual(ManualUiMessage::Nav(ManualMessage::Back))
            }
            (Key::Named(NamedKey::Up), Modifiers::NONE) if !text_focused => {
                Self::scroll_by(-LINE_STEP)
            }
            (Key::Named(NamedKey::Down), Modifiers::NONE) if !text_focused => {
                Self::scroll_by(LINE_STEP)
            }
            (Key::Named(NamedKey::PageUp), Modifiers::NONE) if !text_focused => {
                Self::scroll_by(-page)
            }
            (Key::Named(NamedKey::PageDown), Modifiers::NONE) if !text_focused => {
                Self::scroll_by(page)
            }
            (Key::Named(NamedKey::Home), Modifiers::NONE) if !text_focused => Self::scroll_to(0.0),
            (Key::Named(NamedKey::End), Modifiers::NONE) if !text_focused => {
                operation::snap_to_end(MANUAL_SCROLL_ID)
            }
            _ => match self.app.shortcuts.lookup_in(input, text_focused) {
                Some(Action::Help) => self.toggle_manual(),
                Some(action @ (Action::Quit | Action::FullScreen)) => self.run_action(action),
                _ => Task::none(),
            },
        };
        Task::batch([task, Self::probe_manual_focus()])
    }

    /// An event of the manual window.
    pub(super) fn update_manual(&mut self, message: ManualUiMessage) -> Task<Message> {
        match message {
            ManualUiMessage::Open => {
                let (chapter, anchor) = if self.manual_layer_open() {
                    let place = self.app.manual().place();
                    (place.chapter, place.anchor)
                } else {
                    (0, None)
                };
                self.open_manual_at(chapter, anchor)
            }
            ManualUiMessage::OpenContext(context) => {
                let (chapter, anchor) = help_target(context);
                self.open_manual_at(chapter, anchor.map(str::to_owned))
            }
            ManualUiMessage::Nav(message) => {
                // The place the reader leaves keeps its scroll position.
                let remember = self.dispatch(manual(ManualMessage::RememberScroll(
                    self.ui.dialogs.manual.scroll,
                )));
                let go = self.dispatch(manual(message));
                Task::batch([remember, go])
            }
            ManualUiMessage::Plain(message) => {
                let closing = matches!(message, ManualMessage::Close);
                let task = self.dispatch(manual(message));
                if closing {
                    self.ui.dialogs.manual.search_focused = false;
                }
                task
            }
            ManualUiMessage::Scrolled { offset, height } => {
                self.ui.dialogs.manual.scroll = offset;
                self.ui.dialogs.manual.viewport = height;
                Task::none()
            }
            ManualUiMessage::ScrollNow => {
                let place = self.app.manual().place();
                match place.anchor {
                    Some(anchor) => {
                        let heading = heading_id(&anchor);
                        selector::find(selector::id(iced::widget::Id::from(heading)))
                            .and_then(|target| {
                                selector::find(selector::id(MANUAL_SCROLL_ID)).map(move |area| {
                                    area.map(|area| target.bounds().y - area.bounds().y)
                                })
                            })
                            .map(|y| Message::ManualUi(ManualUiMessage::AnchorFound(y)))
                    }
                    None => Self::scroll_to(place.scroll),
                }
            }
            ManualUiMessage::AnchorFound(y) => match y {
                Some(y) => Self::scroll_to(y.max(0.0)),
                None => Task::none(),
            },
            ManualUiMessage::FocusFind => operation::focus(MANUAL_FIND_ID),
            ManualUiMessage::SearchFocus(on) => {
                self.ui.dialogs.manual.search_focused = on;
                Task::none()
            }
            ManualUiMessage::Noop => Task::none(),
        }
    }
}
