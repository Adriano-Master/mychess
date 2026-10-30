//! The messages and effects of the sounds (spec M8-11, D2808, D2815).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::bank::{SoundFolder, SoundFolderError, load_folder};
use super::cue::SoundCue;
use super::pack::{PackError, PackId, PackScan, SoundPack, load_pack, scan_user_packs};
use super::prefs::SoundPrefs;
use super::sink::{PlayRequest, perform_play};
use crate::error::RequestId;
use crate::message::Message;

/// What the sounds know about the sounds folder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FolderStatus {
    /// No folder is set.
    #[default]
    None,
    /// The folder is being read.
    Loading,
    /// The folder was read: sounds that serve and files refused.
    Loaded {
        /// Files that serve.
        found: usize,
        /// Known names that are not a good WAV.
        skipped: usize,
    },
    /// The folder could not be read.
    Failed(SoundFolderError),
}

/// What the sounds know about the pack in use (D2902, D2906).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PackStatus {
    /// The synthesized sounds are in use; nothing to load.
    #[default]
    None,
    /// The pack is being read and decoded (the synthesized sounds play
    /// meanwhile).
    Loading,
    /// The pack is loaded.
    Loaded {
        /// How many events have a file in the pack.
        covered: usize,
        /// How many remarks about the pack (see `SoundPack::info`).
        warnings: usize,
    },
    /// The pack could not be loaded; the synthesized sounds play.
    Failed(PackError),
}

/// An event of the sounds.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SoundMessage {
    /// Opens the sounds window.
    Open,
    /// Closes the sounds window.
    Close,
    /// Replaces the sound preferences (limited by the application).
    Set(Box<SoundPrefs>),
    /// Plays a cue now, if the sounds are on (for the flows of other items:
    /// start and end of a game, clock warning).
    Cue(SoundCue),
    /// The "Test" button: plays a sample move even with the sounds off, at
    /// the chosen volume.
    Test,
    /// The answer to [`SoundEffect::LoadFolder`].
    FolderLoaded {
        /// Which load this is; an older one is ignored.
        request: RequestId,
        /// The folder, or why it could not be read.
        result: Result<Arc<SoundFolder>, SoundFolderError>,
    },
    /// A sound was handed to the device (nothing to do).
    Played,
    /// Looks into the packs folder of the user again (when the window
    /// opens, or after the user copied a pack).
    ScanPacks,
    /// The answer to [`SoundEffect::ScanPacks`].
    PacksScanned {
        /// Which scan this is; an older one is ignored.
        request: RequestId,
        /// The packs of the user, or why they could not be listed.
        result: Result<PackScan, PackError>,
    },
    /// The user chose a pack (applies at once, D2907); the pack is loaded
    /// in the background and saved in the preferences.
    SelectPack(PackId),
    /// The answer to [`SoundEffect::LoadPack`].
    PackLoaded {
        /// Which load this is; an older one is ignored.
        request: RequestId,
        /// The pack that was asked for.
        id: PackId,
        /// The decoded pack, or why it could not be loaded.
        result: Result<Arc<SoundPack>, PackError>,
    },
    /// The "Open sounds folder" button: the folder of the user is created
    /// (only now) and its path comes back in [`SoundMessage::UserFolderReady`].
    OpenUserFolderRequested,
    /// The answer to [`SoundEffect::EnsureUserFolder`]: the folder exists.
    /// The front-end opens it with the system (or shows the path to copy);
    /// the path is also in `AppState::sound_user_folder`.
    UserFolderReady(Result<PathBuf, PackError>),
}

/// A side effect of the sounds.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SoundEffect {
    /// Plays the clips on the installed [`SoundSink`](super::SoundSink).
    Play(PlayRequest),
    /// Reads the sounds folder on a blocking thread.
    LoadFolder {
        /// Correlates the answer.
        request: RequestId,
        /// The folder.
        path: PathBuf,
    },
    /// Lists the packs of the user's folder (nothing is decoded).
    ScanPacks {
        /// Correlates the answer.
        request: RequestId,
        /// The packs directory (`None`: the system has no data directory).
        dir: Option<PathBuf>,
    },
    /// Reads and decodes a pack on a blocking thread.
    LoadPack {
        /// Correlates the answer.
        request: RequestId,
        /// The pack.
        id: PackId,
        /// The packs directory of the user, for `user:` ids.
        dir: Option<PathBuf>,
    },
    /// Creates the packs directory of the user.
    EnsureUserFolder {
        /// The packs directory (`None`: the system has none).
        dir: Option<PathBuf>,
    },
}

/// The longest a folder load may take before its answer is an error.
pub(crate) const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs `job` on a blocking thread and gives up after `limit`: a folder on a
/// dead network share (or a swapped FIFO) must not leave the window loading
/// for ever. The stuck thread is abandoned; its late result is dropped.
async fn blocking_within<T, E, F>(
    limit: Duration,
    job: F,
    io: fn(std::io::ErrorKind) -> E,
) -> Result<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    match tokio::time::timeout(limit, tokio::task::spawn_blocking(job)).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(io(std::io::ErrorKind::Other)),
        Err(_) => Err(io(std::io::ErrorKind::TimedOut)),
    }
}

async fn load_within<F>(limit: Duration, load: F) -> Result<SoundFolder, SoundFolderError>
where
    F: FnOnce() -> Result<SoundFolder, SoundFolderError> + Send + 'static,
{
    blocking_within(limit, load, SoundFolderError::Io).await
}

/// Runs `effect`. Playing never fails: no device is silence.
pub async fn perform(effect: SoundEffect) -> Message {
    match effect {
        SoundEffect::Play(request) => {
            perform_play(request, None);
            Message::Sound(SoundMessage::Played)
        }
        SoundEffect::LoadFolder { request, path } => {
            let result = load_within(LOAD_TIMEOUT, move || load_folder(&path)).await;
            Message::Sound(SoundMessage::FolderLoaded {
                request,
                result: result.map(Arc::new),
            })
        }
        SoundEffect::ScanPacks { request, dir } => {
            let result = blocking_within(
                LOAD_TIMEOUT,
                move || dir.map_or_else(|| Ok(PackScan::default()), |d| scan_user_packs(&d)),
                PackError::Io,
            )
            .await;
            Message::Sound(SoundMessage::PacksScanned { request, result })
        }
        SoundEffect::LoadPack { request, id, dir } => {
            let wanted = id.clone();
            let result = blocking_within(
                LOAD_TIMEOUT,
                move || load_pack(&wanted, dir.as_deref()),
                PackError::Io,
            )
            .await;
            Message::Sound(SoundMessage::PackLoaded {
                request,
                id,
                result: result.map(Arc::new),
            })
        }
        SoundEffect::EnsureUserFolder { dir } => {
            let result = blocking_within(
                LOAD_TIMEOUT,
                move || {
                    let dir = dir.ok_or(PackError::Io(std::io::ErrorKind::NotFound))?;
                    std::fs::create_dir_all(&dir).map_err(|e| PackError::Io(e.kind()))?;
                    Ok(dir)
                },
                PackError::Io,
            )
            .await;
            Message::Sound(SoundMessage::UserFolderReady(result))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::bank::Clip;
    use super::*;

    #[tokio::test]
    async fn playing_without_a_sink_is_a_quiet_no_op() {
        let request = PlayRequest {
            clips: vec![Arc::new(Clip {
                sample_rate: 8_000,
                channels: 1,
                samples: vec![0.1],
            })],
            gain: 1.0,
        };
        let message = perform(SoundEffect::Play(request)).await;
        assert_eq!(message, Message::Sound(SoundMessage::Played));
    }

    #[tokio::test]
    async fn a_load_that_hangs_answers_a_timeout() {
        let result = load_within(Duration::from_millis(20), || {
            std::thread::sleep(Duration::from_millis(500));
            Err(SoundFolderError::Io(std::io::ErrorKind::Other))
        })
        .await;
        assert_eq!(
            result.unwrap_err(),
            SoundFolderError::Io(std::io::ErrorKind::TimedOut)
        );
    }

    #[tokio::test]
    async fn loading_a_bundled_pack_answers_its_clips() {
        let id = PackId::Bundled("nes".into());
        let message = perform(SoundEffect::LoadPack {
            request: RequestId(9),
            id: id.clone(),
            dir: None,
        })
        .await;
        match message {
            Message::Sound(SoundMessage::PackLoaded {
                request,
                id: got,
                result,
            }) => {
                assert_eq!((request, got), (RequestId(9), id));
                assert_eq!(result.unwrap().info().covered, 6);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn scanning_and_ensuring_the_user_folder() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("sounds");
        let ready = perform(SoundEffect::EnsureUserFolder {
            dir: Some(dir.clone()),
        })
        .await;
        assert_eq!(
            ready,
            Message::Sound(SoundMessage::UserFolderReady(Ok(dir.clone())))
        );
        std::fs::create_dir(dir.join("mine")).unwrap();
        let scanned = perform(SoundEffect::ScanPacks {
            request: RequestId(1),
            dir: Some(dir),
        })
        .await;
        match scanned {
            Message::Sound(SoundMessage::PacksScanned { result, .. }) => {
                assert_eq!(result.unwrap().packs.len(), 1);
            }
            other => panic!("unexpected {other:?}"),
        }
        let no_dir = perform(SoundEffect::EnsureUserFolder { dir: None }).await;
        assert!(matches!(
            no_dir,
            Message::Sound(SoundMessage::UserFolderReady(Err(_)))
        ));
    }

    #[tokio::test]
    async fn loading_a_missing_folder_answers_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing");
        let message = perform(SoundEffect::LoadFolder {
            request: RequestId(3),
            path: path.clone(),
        })
        .await;
        match message {
            Message::Sound(SoundMessage::FolderLoaded { request, result }) => {
                assert_eq!(request, RequestId(3));
                assert_eq!(result.unwrap_err(), SoundFolderError::NotAFolder(path));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
