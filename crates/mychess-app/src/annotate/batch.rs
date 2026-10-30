//! The disk side of the batch annotation over a filter (spec M4-6, phase B,
//! D479, D1520-D1529).
//!
//! The batch walks the games of the filter one at a time. Between two games
//! one blocking step saves the annotated game in the `.si4` file (when there
//! is one), reopens the database and reads the next game. The step runs in
//! `perform` on a blocking thread; `update` only sees its [`BatchStep`].

use std::path::Path;

use mychess_db::GameSource;
use mychess_db::si4::{FlagOp, GameFlags, Si4Database, Si4Writer};

use crate::bases::{BaseError, BaseSource, FileStamp, SharedSource};
use crate::shared::SharedGame;

/// A game to write back to its place in the database.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchSave {
    /// The number of the game (`1..=total`).
    pub number: u32,
    /// The annotated game.
    pub game: SharedGame,
    /// Whether the run marked tactical exercises: the `T` flag is set after
    /// the game is replaced (D471).
    pub tactics: bool,
}

/// What a batch step found on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchStep {
    /// The database opened again after the write.
    pub source: SharedSource,
    /// The state of the file after the write.
    pub stamp: Option<FileStamp>,
    /// The next game, if one was asked for.
    pub game: Option<SharedGame>,
}

/// Saves `save` (if any) in the `.si4` file at `path`, opens the database
/// again and reads game `next` (if any).
///
/// Before anything is written the file must still be the one the slot read:
/// its stamp and the number of games must match, else nothing is written and
/// the error is [`BaseError::Stale`] (D573). The short-lived writer is
/// dropped before the reader opens the files again (Windows locks).
///
/// # Errors
///
/// [`BaseError::Stale`] when the file changed; the writer errors as
/// [`BaseError`]; [`BaseError::GameUnreadable`] when `next` cannot be read.
pub fn batch_step(
    path: &Path,
    save: Option<&BatchSave>,
    next: Option<u32>,
    expected_total: u32,
    stamp: Option<FileStamp>,
) -> Result<BatchStep, BaseError> {
    if stamp.is_some_and(|expected| FileStamp::of(path) != Some(expected)) {
        return Err(BaseError::Stale);
    }
    if let Some(save) = save {
        let mut writer = Si4Writer::open(path).map_err(|e| BaseError::from_si4(&e))?;
        if writer.len() != expected_total {
            return Err(BaseError::Stale);
        }
        writer.replace_game(save.number, &save.game).map_err(|e| {
            tracing::warn!(error = ?e, game = save.number, "a batch game was refused");
            BaseError::from_si4(&e)
        })?;
        if save.tactics {
            writer
                .set_flag(save.number, GameFlags::TACTICS, FlagOp::Set)
                .map_err(|e| BaseError::from_si4(&e))?;
        }
        drop(writer);
    }
    let mut db = Si4Database::open(path).map_err(|e| BaseError::from_si4(&e))?;
    let game = match next {
        Some(number) => Some(
            GameSource::game(&mut db, number)
                .map(SharedGame::new)
                .map_err(|_| BaseError::GameUnreadable { number })?,
        ),
        None => None,
    };
    Ok(BatchStep {
        source: SharedSource::new(BaseSource::Si4(Box::new(db))),
        stamp: FileStamp::of(path),
        game,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::Game;

    fn base_with(path: &Path, games: u32) {
        let mut writer = Si4Writer::create(path, 1).expect("create");
        for _ in 0..games {
            writer
                .add_game(&Game::new(), GameFlags::empty())
                .expect("add");
        }
    }

    fn annotated() -> Game {
        let mut game = Game::new();
        game.tags_mut().set_extra("Annotator", "Fake").ok();
        game
    }

    #[test]
    fn saves_the_game_and_reads_the_next_one() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("b.si4");
        base_with(&path, 3);
        let stamp = FileStamp::of(&path);
        let save = BatchSave {
            number: 1,
            game: SharedGame::new(annotated()),
            tactics: true,
        };
        let step = batch_step(&path, Some(&save), Some(2), 3, stamp).expect("step");
        assert!(step.game.is_some());
        let mut source = step.source.lock();
        let first = GameSource::game(&mut *source, 1).expect("game 1");
        assert_eq!(first.tags().get_extra("Annotator"), Some("Fake"));
        let entry = GameSource::entry(&*source, 1).expect("entry");
        assert!(entry.flags.contains(GameFlags::TACTICS));
    }

    #[test]
    fn refuses_a_file_that_changed_behind_the_back() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("b.si4");
        base_with(&path, 2);
        let stamp = FileStamp::of(&path);
        let save = BatchSave {
            number: 1,
            game: SharedGame::new(annotated()),
            tactics: false,
        };
        // The slot believes there are 5 games.
        let error = batch_step(&path, Some(&save), None, 5, stamp).expect_err("stale");
        assert_eq!(error, BaseError::Stale);
    }

    #[test]
    fn a_step_without_save_only_reads() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("b.si4");
        base_with(&path, 2);
        let step = batch_step(&path, None, Some(2), 2, FileStamp::of(&path)).expect("step");
        assert!(step.game.is_some());
        let error = batch_step(&path, None, Some(9), 2, None).expect_err("missing game");
        assert_eq!(error, BaseError::GameUnreadable { number: 9 });
    }
}
