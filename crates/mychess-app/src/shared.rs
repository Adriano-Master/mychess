//! A cheaply clonable, comparable handle to a [`Game`].
//!
//! `Message` and `Effect` must be `Clone + PartialEq` (the front-end clones
//! them and the tests compare them), but `Game` is neither cheap to clone nor
//! `PartialEq`. [`SharedGame`] wraps it in an `Arc` and compares by content
//! (`Game::same_content`).

use std::ops::Deref;
use std::sync::Arc;

use mychess_core::{EcoBook, Game};

/// A shared, immutable [`Game`] compared by content.
#[derive(Debug, Clone)]
pub struct SharedGame(Arc<Game>);

impl SharedGame {
    /// Wraps `game`.
    pub fn new(game: Game) -> SharedGame {
        SharedGame(Arc::new(game))
    }

    /// An owned copy of the game.
    pub fn to_game(&self) -> Game {
        (*self.0).clone()
    }
}

impl From<Game> for SharedGame {
    fn from(game: Game) -> SharedGame {
        SharedGame::new(game)
    }
}

impl Deref for SharedGame {
    type Target = Game;

    fn deref(&self) -> &Game {
        &self.0
    }
}

impl PartialEq for SharedGame {
    fn eq(&self, other: &SharedGame) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.same_content(&other.0)
    }
}

impl Eq for SharedGame {}

/// A shared opening book, compared by identity.
#[derive(Debug, Clone)]
pub struct SharedEco(Arc<EcoBook>);

impl SharedEco {
    /// Wraps `book`.
    pub fn new(book: EcoBook) -> SharedEco {
        SharedEco(Arc::new(book))
    }
}

impl Deref for SharedEco {
    type Target = EcoBook;

    fn deref(&self) -> &EcoBook {
        &self.0
    }
}

impl PartialEq for SharedEco {
    fn eq(&self, other: &SharedEco) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedEco {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_by_content() {
        let a = SharedGame::new(Game::new());
        let b = SharedGame::new(Game::new());
        assert_eq!(a, b);
        let mut other = Game::new();
        other.tags_mut().white = "X".to_owned();
        assert_ne!(a, SharedGame::new(other));
    }
}
