//! The tasks of the tournament that `perform` runs (spec M8-2): reading the
//! book and the suite, talking to the engines, writing the PGN and the
//! state.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use super::config::{BookPick, OpeningSource, TournamentConfig};
use super::message::{LoadFailure, Loaded, SlotWrite, StoreFailure, TournamentMessage};
use super::openings::{load_suite, mix};
use super::store::{
    list_unfinished, load_state, make_slug, pgn_path, save_state, state_path, tournaments_dir,
};
use super::tablebase::TournamentTablebase;
use mychess_db::si4::{GameFlags, Si4Database, Si4Writer};
use mychess_tablebase::LocalTablebase;

use super::resources::SystemResources;
use crate::bases::{BaseError, BaseSource, FileStamp, Reloaded, SharedSource};
use crate::effect::{Effect, today_utc};
use crate::files::{SaveMode, save_pgn};
use crate::fsutil::write_atomic;
use crate::message::Message;
use crate::play::{BookSelection, read_book};
use crate::prefs::PgnExportPrefs;
use crate::shared::SharedGame;

/// Runs blocking `work` off the async executor.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    on_panic: impl FnOnce() -> T,
) -> T {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| on_panic())
}

/// The revision of the last state written to each file. The lock is held for
/// the whole write, so the writes of the tournament (PGN appends, which must not interleave, and the state) never overlap: two appends at the
/// same time would lose a game, two state writes could leave the older one on
/// disk (D1306). One lock for every tournament of the process is enough, the
/// writes are short and one tournament runs at a time.
static WRITTEN: tokio::sync::Mutex<BTreeMap<PathBuf, u64>> =
    tokio::sync::Mutex::const_new(BTreeMap::new());

/// Adds one game to an `.si4` file and opens the database again (D2519).
/// The file must be what the slot read (stamp and number of games), else
/// nothing is written.
fn add_to_si4(
    path: &std::path::Path,
    expected_total: u32,
    stamp: Option<FileStamp>,
    game: &SharedGame,
) -> SlotWrite {
    add_and_reopen(path, expected_total, stamp, game, |path| {
        let db = Si4Database::open(path).map_err(|e| BaseError::from_si4(&e))?;
        Ok(Reloaded {
            source: SharedSource::new(BaseSource::Si4(Box::new(db))),
            stamp: FileStamp::of(path),
        })
    })
}

/// The writing and the reopening of [`add_to_si4`], with the reopening given,
/// so the two failures can be told apart (D2519): when the game is on disk
/// but the database does not open, the slot is out of date.
fn add_and_reopen(
    path: &std::path::Path,
    expected_total: u32,
    stamp: Option<FileStamp>,
    game: &SharedGame,
    reopen: impl FnOnce(&std::path::Path) -> Result<Reloaded, BaseError>,
) -> SlotWrite {
    if stamp.is_some_and(|expected| FileStamp::of(path) != Some(expected)) {
        return SlotWrite::NotWritten(BaseError::Stale);
    }
    {
        let mut writer = match Si4Writer::open(path) {
            Ok(writer) => writer,
            Err(e) => return SlotWrite::NotWritten(BaseError::from_si4(&e)),
        };
        if writer.len() != expected_total {
            return SlotWrite::NotWritten(BaseError::Stale);
        }
        if let Err(e) = writer.add_game(game, GameFlags::empty()) {
            return SlotWrite::NotWritten(BaseError::from_si4(&e));
        }
        // On Windows a live writer blocks the reader: it goes before the
        // database opens again.
    }
    match reopen(path) {
        Ok(reloaded) => SlotWrite::Reopened(reloaded),
        Err(error) => SlotWrite::WrittenNotReopened(error),
    }
}

fn message(message: TournamentMessage) -> Message {
    Message::Tournament(message)
}

fn load(
    config: &TournamentConfig,
    slug: Option<String>,
    dir: Option<PathBuf>,
    tablebase_dirs: &[PathBuf],
) -> Result<Loaded, LoadFailure> {
    config.check_files().map_err(LoadFailure::Config)?;
    let dir = dir.or_else(tournaments_dir).ok_or(LoadFailure::Directory)?;
    std::fs::create_dir_all(&dir).map_err(|_| LoadFailure::Directory)?;
    let today = today_utc();
    let (book, book_unreadable) = match (&config.opening, &config.book) {
        (OpeningSource::Book, Some(choice)) => {
            let selection = match choice.pick {
                BookPick::Best => BookSelection::Best,
                BookPick::Weighted => BookSelection::Weighted,
            };
            match read_book(&choice.path, selection) {
                Ok(book) => (Some(book), false),
                Err(_) => (None, true),
            }
        }
        _ => (None, false),
    };
    let suite = match (&config.opening, &config.suite) {
        (OpeningSource::Suite, Some(choice)) => {
            Some(load_suite(&choice.path, choice.plies).map_err(|_| LoadFailure::Suite)?)
        }
        _ => None,
    };
    let slug = slug.unwrap_or_else(|| {
        make_slug(&config.name, today, |candidate| {
            state_path(&dir, candidate).exists() || pgn_path(&dir, candidate).exists()
        })
    });
    let seed = if config.seed == 0 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| {
                u64::try_from(d.as_nanos() & u128::from(u64::MAX)).unwrap_or(1)
            });
        // A signed 64-bit number: the files are TOML.
        (mix(nanos, 1, 2) >> 1).max(1)
    } else {
        config.seed
    };
    let tablebase = if config.adjudication.tablebase && !tablebase_dirs.is_empty() {
        open_tablebase(tablebase_dirs)
    } else {
        None
    };
    Ok(Loaded {
        book,
        book_unreadable,
        suite,
        slug,
        dir,
        today,
        seed,
        tablebase,
    })
}

/// Opens the Syzygy folders for the adjudication (D2538); `None` when
/// nothing usable is there (a folder that cannot be read, or no table at
/// all), which the flow reports as a notice.
fn open_tablebase(dirs: &[PathBuf]) -> Option<TournamentTablebase> {
    match LocalTablebase::open(dirs) {
        Ok((tables, _report)) if !tables.is_empty() => Some(TournamentTablebase::new(tables)),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(
                ?error,
                "the tablebase folders of the tournament could not be opened"
            );
            None
        }
    }
}

/// Executes the effects of the tournament (called by `perform`).
#[allow(clippy::too_many_lines)] // one flat match over the effects reads best
pub(crate) async fn perform_tournament(effect: Effect) -> Message {
    match effect {
        Effect::TournamentLoad {
            request,
            config,
            slug,
            dir,
            tablebase_dirs,
        } => {
            let result = blocking(
                move || load(&config, slug, dir, &tablebase_dirs),
                || Err(LoadFailure::Directory),
            )
            .await;
            message(TournamentMessage::Loaded {
                request,
                result: Box::new(result),
                at: Instant::now(),
            })
        }
        Effect::TournamentPrepare { pool, plan, epoch } => {
            let result = pool.prepare(&plan, epoch).await;
            message(TournamentMessage::Prepared {
                epoch,
                result,
                at: Instant::now(),
            })
        }
        Effect::TournamentSearch { pool, job } => {
            let epoch = job.epoch;
            let outcome = pool.search(*job).await;
            message(TournamentMessage::Searched {
                epoch,
                outcome,
                at: Instant::now(),
            })
        }
        Effect::TournamentProbe => {
            let resources = blocking(SystemResources::probe, SystemResources::default).await;
            message(TournamentMessage::Resources(resources))
        }
        Effect::TournamentAddToSlot {
            path,
            expected_total,
            stamp,
            number,
            game,
        } => {
            // No lock of the PGN and of the state: those writes must not
            // wait for a slow database. The next copy starts only after this
            // answer (`dest.busy`, D2519).
            let result = blocking(
                move || add_to_si4(&path, expected_total, stamp, &game),
                || SlotWrite::NotWritten(BaseError::Internal),
            )
            .await;
            tracing::debug!(number, "tournament game copied to a slot");
            message(TournamentMessage::SlotAdded {
                result: Box::new(result),
            })
        }
        Effect::TournamentPool { pool, command } => {
            pool.command(command).await;
            message(TournamentMessage::PoolDone)
        }
        Effect::TournamentSaveGame { path, number, game } => {
            let _writing = WRITTEN.lock().await;
            let ok = blocking(
                move || {
                    save_pgn(
                        &path,
                        SaveMode::AppendInPlace,
                        &game,
                        &PgnExportPrefs::default(),
                    )
                    .map_err(|error| {
                        tracing::warn!(?error, "could not append a tournament game");
                    })
                    .is_ok()
                },
                || false,
            )
            .await;
            message(TournamentMessage::GameSaved { number, ok })
        }
        Effect::TournamentSaveState { path, state } => {
            let revision = state.revision;
            let mut written = WRITTEN.lock().await;
            // A newer state is already on disk: this one is stale.
            if written.get(&path).is_some_and(|&last| last > revision) {
                return message(TournamentMessage::StateSaved { ok: true });
            }
            let key = path.clone();
            let ok = blocking(
                move || {
                    save_state(&path, &state)
                        .map_err(|error| {
                            tracing::warn!(?error, "could not save the tournament state");
                        })
                        .is_ok()
                },
                || false,
            )
            .await;
            if ok {
                written.insert(key, revision);
            }
            message(TournamentMessage::StateSaved { ok })
        }
        Effect::TournamentLoadState { request, path } => {
            let result = blocking(
                move || load_state(&path).map_err(|e| StoreFailure::from(&e)),
                || Err(StoreFailure::Io),
            )
            .await;
            message(TournamentMessage::StateLoaded {
                request,
                result: Box::new(result),
                at: Instant::now(),
            })
        }
        Effect::TournamentList { dir } => {
            let list = blocking(
                move || {
                    dir.or_else(tournaments_dir)
                        .map(|d| list_unfinished(&d))
                        .unwrap_or_default()
                },
                Vec::new,
            )
            .await;
            message(TournamentMessage::Resumable(list))
        }
        Effect::TournamentExport { path, text } => {
            let ok = blocking(
                move || {
                    write_atomic(&path, |file| file.write_all(text.as_bytes()))
                        .map_err(|error| {
                            tracing::warn!(?error, "could not export a tournament table");
                        })
                        .is_ok()
                },
                || false,
            )
            .await;
            message(TournamentMessage::Exported { ok })
        }
        _ => Message::HostEffectNotHandled,
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::{Game, PartialDate};

    use super::*;
    use crate::shared::SharedGame;
    use crate::tournament::config::Participant;
    use crate::tournament::runner::Tournament;
    use crate::tournament::store::{SavedTournament, load_state};

    fn saved(revision: u64, failures: u32) -> Box<SavedTournament> {
        let config = TournamentConfig {
            participants: vec![Participant::new(1, "A"), Participant::new(2, "B")],
            ..TournamentConfig::default()
        };
        let t = Tournament::new(config, "t", PartialDate::default(), None, None).expect("t");
        let mut state = t.to_saved();
        state.revision = revision;
        state.failures = vec![failures, 0];
        Box::new(state)
    }

    #[tokio::test]
    async fn a_copy_to_a_slot_does_not_wait_for_the_lock_of_the_pgn_and_the_state() {
        // D2519: the writes of the tournament must not wait for a slow
        // database. The lock is held here; the copy still answers.
        let _writing = WRITTEN.lock().await;
        let effect = Effect::TournamentAddToSlot {
            path: PathBuf::from("does-not-exist.si4"),
            expected_total: 0,
            stamp: None,
            number: 1,
            game: SharedGame::new(Game::new()),
        };
        let answer = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            perform_tournament(effect),
        )
        .await
        .expect("no wait for the lock");
        assert!(matches!(
            answer,
            Message::Tournament(TournamentMessage::SlotAdded { .. })
        ));
    }

    #[test]
    fn a_game_written_to_the_file_is_told_apart_from_one_that_was_not() {
        use mychess_db::si4::Si4Writer;
        let dir = tempfile::tempdir().expect("dir");
        let base = dir.path().join("b");
        drop(Si4Writer::create(&base, 0).expect("create"));
        let path = base.with_extension("si4");
        let game = SharedGame::new(Game::new());
        // The reopening fails after the game is on disk.
        let written = add_and_reopen(&path, 0, None, &game, |_| Err(BaseError::Internal));
        assert_eq!(written, SlotWrite::WrittenNotReopened(BaseError::Internal));
        let reader = Si4Database::open(&path).expect("open");
        assert_eq!(mychess_db::GameSource::len(&reader), 1, "the game is there");
        drop(reader);
        // Nothing is written when the file is not what the slot read.
        let stale = add_and_reopen(&path, 0, None, &game, |_| Err(BaseError::Internal));
        assert_eq!(stale, SlotWrite::NotWritten(BaseError::Stale));
        let reader = Si4Database::open(&path).expect("open");
        assert_eq!(mychess_db::GameSource::len(&reader), 1);
        // The normal way reopens the database.
        drop(reader);
        let ok = add_to_si4(&path, 1, None, &game);
        assert!(matches!(ok, SlotWrite::Reopened(_)), "{ok:?}");
    }

    #[tokio::test]
    async fn a_stale_state_does_not_replace_a_newer_one() {
        // Regression (D1306): two saves in flight could land in any order.
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("stale-order.toml");
        for (revision, failures) in [(10, 1), (8, 7), (11, 2)] {
            let effect = Effect::TournamentSaveState {
                path: path.clone(),
                state: saved(revision, failures),
            };
            let answer = perform_tournament(effect).await;
            assert_eq!(
                answer,
                Message::Tournament(TournamentMessage::StateSaved { ok: true })
            );
        }
        // Revision 8 was dropped and 11 won.
        assert_eq!(load_state(&path).expect("load").failures, [2, 0]);
        let effect = Effect::TournamentSaveState {
            path: path.clone(),
            state: saved(9, 9),
        };
        perform_tournament(effect).await;
        assert_eq!(load_state(&path).expect("load").failures, [2, 0]);
    }

    #[tokio::test]
    async fn games_appended_at_the_same_time_are_all_kept() {
        // Regression (D1306): `Append` reads the PGN and replaces it, so two
        // at once lost a game.
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("concurrent.pgn");
        let mut tasks = Vec::new();
        for number in 1..=12u32 {
            let effect = Effect::TournamentSaveGame {
                path: path.clone(),
                number,
                game: SharedGame::new(Game::new()),
            };
            tasks.push(tokio::spawn(perform_tournament(effect)));
        }
        for task in tasks {
            let answer = task.await.expect("task");
            assert!(
                matches!(
                    answer,
                    Message::Tournament(TournamentMessage::GameSaved { ok: true, .. })
                ),
                "{answer:?}"
            );
        }
        let text = std::fs::read_to_string(&path).expect("pgn");
        assert_eq!(text.matches("[Event ").count(), 12, "{text}");
    }
}
