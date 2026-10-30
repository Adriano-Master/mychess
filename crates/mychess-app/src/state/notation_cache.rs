//! The notation document kept in the state (spec M2-4, D160).
//!
//! `view` only has `&AppState`, so the document is rebuilt in `update`:
//! [`AppState::refresh_notation`] runs at the end of every
//! [`update`](super::update) and rebuilds only when the content of the game
//! ([`GameSession::changes`](crate::GameSession::changes)), the options or
//! the pending variation changed. Moving the cursor never rebuilds.

use mychess_core::NodeId;

use super::AppState;
use crate::game_info::OpeningName;
use crate::nav_state::NavState;
use crate::notation::{
    NotationDoc, NotationOptions, NotationTable, build_notation_with_pending, opening_along_path,
};
use crate::shared::SharedEco;

/// What the cached document was built from.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    /// The root names the game (a replaced game has another root even if
    /// the change counter happens to repeat).
    root: NodeId,
    changes: u64,
    pending: Option<NodeId>,
    options: NotationOptions,
}

/// What the cached opening name was computed from (it depends on the cursor,
/// unlike the document).
#[derive(Debug, Clone, PartialEq)]
struct OpeningKey {
    root: NodeId,
    changes: u64,
    cursor: NodeId,
    book: Option<SharedEco>,
}

/// The cached document, its table and the number of rebuilds so far.
#[derive(Debug, Default)]
pub(crate) struct NotationCache {
    doc: NotationDoc,
    table: NotationTable,
    key: Option<Key>,
    revision: u64,
    opening: Option<OpeningName>,
    opening_key: Option<OpeningKey>,
}

impl AppState {
    /// The notation document for the game and the notation preferences.
    ///
    /// Up to date after every [`update`](crate::update). If the session was
    /// changed directly (`state.session.update(..)`), call
    /// [`AppState::refresh_notation`] first.
    pub fn notation(&self) -> &NotationDoc {
        &self.notation_cache.doc
    }

    /// The document projected as rows (`number | white | black`), rebuilt
    /// together with [`AppState::notation`]; moving the cursor never
    /// rebuilds it (M2-5 D242). Same freshness as `notation`.
    pub fn notation_table(&self) -> &NotationTable {
        &self.notation_cache.table
    }

    /// The opening of the notation header: the last entry of the opening
    /// book on the path from the start to the cursor
    /// ([`opening_along_path`]); `None` without a book or a match. Same
    /// freshness as `notation`.
    pub fn opening(&self) -> Option<&OpeningName> {
        self.notation_cache
            .opening
            .as_ref()
            .filter(|_| !self.assist_locked())
    }

    /// The options the notation panel is drawn with.
    pub fn notation_options(&self) -> &NotationOptions {
        &self.prefs.notation
    }

    /// The state of the navigation bar for the cursor position.
    pub fn nav_state(&self) -> NavState {
        NavState::of(&self.session)
    }

    /// Rebuilds the notation document if what it depends on changed. Cheap
    /// (a comparison) when nothing did.
    pub fn refresh_notation(&mut self) {
        self.refresh_opening();
        let key = Key {
            root: self.session.game().root(),
            changes: self.session.changes(),
            pending: self.session.pending_variation(),
            options: self.prefs.notation,
        };
        if self.notation_cache.key.as_ref() == Some(&key) {
            return;
        }
        let revision = self.notation_cache.revision.saturating_add(1);
        let game = self.session.game();
        let mut doc = build_notation_with_pending(game, &key.options, revision, key.pending);
        doc.keep_revisions(&self.notation_cache.doc);
        let mut table = NotationTable::from_doc(&doc, game, &key.options);
        table.keep_revisions(&self.notation_cache.table);
        self.notation_cache = NotationCache {
            doc,
            table,
            key: Some(key),
            revision,
            opening: self.notation_cache.opening.take(),
            opening_key: self.notation_cache.opening_key.take(),
        };
    }

    /// Recomputes the opening name when the cursor, the game or the book
    /// changed.
    fn refresh_opening(&mut self) {
        let key = OpeningKey {
            root: self.session.game().root(),
            changes: self.session.changes(),
            cursor: self.session.cursor(),
            book: self.eco_book.clone(),
        };
        if self.notation_cache.opening_key.as_ref() == Some(&key) {
            return;
        }
        self.notation_cache.opening =
            opening_along_path(self.session.game(), key.cursor, self.eco_book.as_deref());
        self.notation_cache.opening_key = Some(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::prefs::{PrefChange, PrefsMessage};
    use crate::session::{GameMessage, Nav};
    use crate::state::update;
    use mychess_core::{Game, Position};

    fn state_with_game() -> AppState {
        let mut game = Game::new();
        let pos = Position::new();
        let e4 = game
            .add_move(game.root(), pos.parse_uci("e2e4").unwrap())
            .unwrap();
        game.add_move(e4, game.position_at(e4).unwrap().parse_uci("e7e5").unwrap())
            .unwrap();
        let mut state = AppState::default();
        state.session.replace_game(game);
        state.refresh_notation();
        state
    }

    fn send(state: &mut AppState, message: GameMessage) {
        update(state, Message::Game(message));
    }

    #[test]
    fn new_state_shows_the_empty_game() {
        let state = AppState::default();
        assert!(state.notation().blocks.is_empty());
        assert_eq!(state.notation_options(), &NotationOptions::default());
    }

    #[test]
    fn replaced_game_is_shown() {
        let state = state_with_game();
        assert_eq!(state.notation().flat_text(), "1.e4 e5 *");
    }

    #[test]
    fn moving_the_cursor_does_not_rebuild() {
        let mut state = state_with_game();
        let revision = state.notation().revision;
        send(&mut state, GameMessage::Navigate(Nav::Start));
        send(&mut state, GameMessage::Navigate(Nav::GameEnd));
        assert_eq!(state.notation().revision, revision);
    }

    #[test]
    fn editing_rebuilds_and_only_changed_blocks_get_a_new_revision() {
        let mut state = state_with_game();
        let before = state.notation().revision;
        send(&mut state, GameMessage::Navigate(Nav::GameEnd));
        let node = state.session.cursor();
        send(
            &mut state,
            GameMessage::SetComment {
                node,
                text: "hi".to_owned(),
            },
        );
        let doc = state.notation();
        assert!(doc.revision > before);
        // The comment block is new; the move block before it is not.
        assert!(doc.blocks.iter().any(|b| b.rev == doc.revision));
        assert!(doc.flat_text().contains("hi"));
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn changing_the_options_rebuilds() {
        let mut state = state_with_game();
        let revision = state.notation().revision;
        let mut options = NotationOptions::default();
        options.show_result = false;
        update(
            &mut state,
            Message::Prefs(PrefsMessage::Set(PrefChange::Notation(options))),
        );
        assert!(state.notation().revision > revision);
        assert_eq!(state.notation().result, None);
    }

    #[test]
    fn armed_variation_appears_and_disappears() {
        let mut state = state_with_game();
        send(&mut state, GameMessage::Navigate(Nav::Start));
        send(&mut state, GameMessage::StartVariation);
        assert!(state.notation().flat_text().contains("( ... )"));
        send(&mut state, GameMessage::Navigate(Nav::GameEnd));
        assert!(!state.notation().flat_text().contains("( ... )"));
    }

    #[test]
    fn table_follows_the_document_and_ignores_the_cursor() {
        let mut state = state_with_game();
        let table = state.notation_table().clone();
        assert_eq!(table.revision, state.notation().revision);
        assert_eq!(table.rows.len(), 2, "one move row and the result");
        send(&mut state, GameMessage::Navigate(Nav::Start));
        send(&mut state, GameMessage::Navigate(Nav::GameEnd));
        assert_eq!(state.notation_table(), &table);
        let node = state.session.cursor();
        send(
            &mut state,
            GameMessage::SetComment {
                node,
                text: "hi".to_owned(),
            },
        );
        assert!(state.notation_table().revision > table.revision);
        assert_eq!(state.notation_table().revision, state.notation().revision);
        // The move row did not change: it keeps its revision (D254).
        assert_eq!(state.notation_table().rows[0].rev, table.rows[0].rev);
    }

    #[test]
    fn opening_follows_the_cursor_and_the_book() {
        use crate::shared::SharedEco;
        use mychess_core::EcoBook;
        let mut state = state_with_game();
        assert_eq!(state.opening(), None, "no book");
        let book = EcoBook::parse("C20 \"King's Pawn Game\" 1.e4 e5 *\n").unwrap();
        state.eco_book = Some(SharedEco::new(book));
        send(&mut state, GameMessage::Navigate(Nav::GameEnd));
        assert_eq!(
            state.opening().map(|o| o.name.as_str()),
            Some("King's Pawn Game")
        );
        send(&mut state, GameMessage::Navigate(Nav::Start));
        assert_eq!(state.opening(), None, "the start names nothing");
        send(&mut state, GameMessage::Navigate(Nav::GameEnd));
        state.eco_book = None;
        state.refresh_notation();
        assert_eq!(state.opening(), None);
    }
}
