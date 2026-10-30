//! Messages, effects and notices of the player information window.

use mychess_db::Filter;
use mychess_db::finder::Cancelled;
use mychess_db::player_info::{
    GameSelection, PlayerFilterMode, PlayerInfo, SpellingHeader, fide_profile_url,
    player_games_filter, player_info, spelling_header,
};
use mychess_db::spelling::photo::{PhotoSource, detect_kind};

use super::view::{InfoHeader, PhotoFormat, PlayerPhoto};
use crate::Message;
use crate::bases::{OpenControl, SharedSource};
use crate::error::RequestId;
use crate::spelling_ui::{SharedPhotos, SharedSpelling};
use crate::tree::LockingSource;

/// Milliseconds the window waits for a burst of changes to end before it
/// counts again (D3318).
pub(crate) const DEBOUNCE_MS: u32 = 200;

/// The largest photo file read.
const MAX_PHOTO_BYTES: u64 = 4 << 20;

/// What a count of the player's games found.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoDone {
    /// The figures (empty when the player is not in the database).
    pub info: PlayerInfo,
    /// What the spelling file says.
    pub header: Option<InfoHeader>,
    /// The photo.
    pub photo: Option<PlayerPhoto>,
}

/// The kind of filter link of the window (`-f`, `-F`, `-o`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Link {
    pub(crate) mode: PlayerFilterMode,
    pub(crate) selection: GameSelection,
}

/// Reads a link code of the Scid: `-f<letters>`, `-F<letters>` or
/// `-o<letters>`; the letters are `a` `A` `w` `W` `d` `D` `l` `L`. `-F`
/// alone is valid (it only drops the games of other players). `None` for an
/// unknown letter or prefix (the Scid answers with an error of use).
pub(crate) fn parse_link(code: &str) -> Option<Link> {
    let rest = code.strip_prefix('-')?;
    let mut chars = rest.chars();
    let mode = match chars.next()? {
        'f' => PlayerFilterMode::Set,
        'F' => PlayerFilterMode::Refine,
        'o' => PlayerFilterMode::Opponent,
        _ => return None,
    };
    let selection = GameSelection::from_letters(chars.as_str())?;
    Some(Link { mode, selection })
}

/// An event of the player information window.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlayerInfoMessage {
    /// Opens the window on a player (the name in the game information area,
    /// a row of the finder...). An empty name repeats the last player; the
    /// name goes to the history.
    Open {
        /// The exact name.
        name: String,
    },
    /// Closes the window.
    Close,
    /// "Update": counts again at once.
    Refresh,
    /// A link of the statistics: `-fwW`, `-Fl`, `-o`... Sets the filter of
    /// the database (section 3.4).
    FilterLink(String),
    /// Opens the FIDE profile in the browser.
    OpenFide,
    /// "Lookup name": opens the player finder with the first word of the
    /// name.
    LookupName,
    /// "Tournaments": opens the tournament finder for the player.
    OpenTournaments,
    /// Stops the count.
    Cancel,
    /// The wait of [`PlayerInfoEffect::Delay`] ended.
    DelayElapsed(RequestId),
    /// The answer of [`PlayerInfoEffect::Compute`].
    Computed {
        /// Correlates the answer.
        request: RequestId,
        /// The figures, `None` when stopped.
        result: Box<Option<InfoDone>>,
    },
    /// The answer of [`PlayerInfoEffect::SetFilter`].
    FilterReady {
        /// Correlates the answer.
        request: RequestId,
        /// The new filter, `None` when stopped.
        filter: Box<Option<Filter>>,
    },
    /// The answer of [`PlayerInfoEffect::OpenUrl`].
    UrlOpened {
        /// The browser was started.
        ok: bool,
    },
}

/// What `perform` does for the player information.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlayerInfoEffect {
    /// Counts the games of a player; answers [`PlayerInfoMessage::Computed`].
    Compute {
        /// Correlates the answer.
        request: RequestId,
        /// What to count.
        job: Box<InfoJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Sets the filter from a link; answers [`PlayerInfoMessage::FilterReady`].
    SetFilter {
        /// Correlates the answer.
        request: RequestId,
        /// What to do.
        job: Box<FilterJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Waits `ms` milliseconds; answers [`PlayerInfoMessage::DelayElapsed`].
    Delay {
        /// Correlates the answer.
        request: RequestId,
        /// How long.
        ms: u32,
    },
    /// Opens an address in the browser; answers [`PlayerInfoMessage::UrlOpened`].
    OpenUrl {
        /// The address (`https`).
        url: String,
    },
}

/// What the count needs.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoJob {
    /// The database.
    pub source: SharedSource,
    /// The player.
    pub name: String,
    /// A copy of the filter of the slot.
    pub filter: Option<Box<Filter>>,
    /// The opponent of the game on screen.
    pub opponent: Option<String>,
    /// The spelling file.
    pub set: Option<SharedSpelling>,
    /// The photos.
    pub photos: SharedPhotos,
}

/// What the filter link needs.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterJob {
    /// The database.
    pub source: SharedSource,
    /// The player.
    pub name: String,
    /// The games wanted.
    pub selection: GameSelection,
    /// How the games meet the filter.
    pub mode: PlayerFilterMode,
    /// The opponent of the game on screen (for `-o`).
    pub opponent: Option<String>,
    /// A copy of the filter of the slot.
    pub filter: Box<Filter>,
}

/// A remark about the player information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlayerInfoNotice {
    /// There is no database (`pinfo-no-base`).
    NoBase,
    /// There is no player to look up (`pinfo-no-player`).
    NoPlayer,
    /// The link is not one of the window's (`pinfo-bad-link`).
    BadLink,
    /// The player has no FIDE profile in the notes (`pinfo-no-fide`).
    NoFide,
    /// The browser could not be started (`pinfo-fide-failed`).
    FideFailed,
    /// The filter was set (`pinfo-filter-set`).
    FilterSet {
        /// The games in the filter.
        games: u32,
    },
    /// The database changed while the link was worked out
    /// (`finder-stale`).
    Stale,
    /// The work of the window died and the figures on show are the
    /// previous ones (`finder-failed`, D3429).
    Failed,
}

impl PlayerInfoNotice {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            PlayerInfoNotice::NoBase => "pinfo-no-base",
            PlayerInfoNotice::NoPlayer => "pinfo-no-player",
            PlayerInfoNotice::BadLink => "pinfo-bad-link",
            PlayerInfoNotice::NoFide => "pinfo-no-fide",
            PlayerInfoNotice::FideFailed => "pinfo-fide-failed",
            PlayerInfoNotice::FilterSet { .. } => "pinfo-filter-set",
            PlayerInfoNotice::Stale => "finder-stale",
            PlayerInfoNotice::Failed => "finder-failed",
        }
    }
}

fn photo_of(job: &InfoJob) -> Option<PlayerPhoto> {
    let checker = job
        .set
        .as_ref()
        .map(|s| s.checker(mychess_db::si4::NameKind::Player));
    let bytes: std::sync::Arc<[u8]> = match job.photos.lookup(&job.name, checker)? {
        PhotoSource::Bytes(bytes) => bytes.clone(),
        PhotoSource::File(path) => {
            // A device, a pipe or a directory named like a photo must not be
            // opened (it could block).
            if !std::fs::metadata(path).ok()?.is_file() {
                return None;
            }
            let file = std::fs::File::open(path).ok()?;
            let mut buffer = Vec::new();
            std::io::Read::read_to_end(
                &mut std::io::Read::take(file, MAX_PHOTO_BYTES + 1),
                &mut buffer,
            )
            .ok()?;
            if u64::try_from(buffer.len()).ok()? > MAX_PHOTO_BYTES {
                return None;
            }
            buffer.into()
        }
    };
    let format = PhotoFormat::of(detect_kind(&bytes)?);
    Some(PlayerPhoto { bytes, format })
}

fn header_of(job: &InfoJob) -> Option<InfoHeader> {
    let checker = job.set.as_ref()?.checker(mychess_db::si4::NameKind::Player);
    let header: SpellingHeader = spelling_header(checker, &job.name)?;
    let candidates = if header.exact {
        Vec::new()
    } else {
        checker
            .corrections(&job.name, 10)
            .into_iter()
            .map(str::to_owned)
            .collect()
    };
    let url = header.fide_id.as_deref().map(fide_profile_url);
    Some(InfoHeader::of(&header, candidates, url))
}

/// Counts the games of the player and looks the spelling file and the photos
/// up (the work of [`PlayerInfoEffect::Compute`]).
pub(crate) fn compute(job: &InfoJob, control: &OpenControl) -> Result<InfoDone, Cancelled> {
    let db = LockingSource::new(job.source.clone());
    let mut progress = control.reporter();
    let info = player_info(
        &db,
        &job.name,
        job.filter.as_deref(),
        job.opponent.as_deref(),
        &control.token(),
        &mut progress,
    )?;
    Ok(InfoDone {
        info,
        header: header_of(job),
        photo: photo_of(job),
    })
}

/// Sets a copy of the filter from a link.
pub(crate) fn set_filter(job: &FilterJob, control: &OpenControl) -> Result<Filter, Cancelled> {
    let db = LockingSource::new(job.source.clone());
    let mut progress = control.reporter();
    let mut filter = (*job.filter).clone();
    player_games_filter(
        &db,
        &job.name,
        job.selection,
        job.mode,
        job.opponent.as_deref(),
        &mut filter,
        &control.token(),
        &mut progress,
    )?;
    Ok(filter)
}

/// Runs an effect of the player information. Never panics.
pub(crate) async fn perform(effect: PlayerInfoEffect) -> Message {
    let message = match effect {
        PlayerInfoEffect::Compute {
            request,
            job,
            control,
        } => {
            let result = tokio::task::spawn_blocking(move || compute(&job, &control).ok())
                .await
                .unwrap_or(None);
            PlayerInfoMessage::Computed {
                request,
                result: Box::new(result),
            }
        }
        PlayerInfoEffect::SetFilter {
            request,
            job,
            control,
        } => {
            let filter = tokio::task::spawn_blocking(move || set_filter(&job, &control).ok())
                .await
                .unwrap_or(None);
            PlayerInfoMessage::FilterReady {
                request,
                filter: Box::new(filter),
            }
        }
        PlayerInfoEffect::Delay { request, ms } => {
            tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
            PlayerInfoMessage::DelayElapsed(request)
        }
        PlayerInfoEffect::OpenUrl { url } => {
            let ok = tokio::task::spawn_blocking(move || crate::online::open_url(&url))
                .await
                .unwrap_or(false);
            PlayerInfoMessage::UrlOpened { ok }
        }
    };
    Message::PlayerInfo(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_the_codes_of_the_scid() {
        let l = parse_link("-fwW").expect("link");
        assert_eq!(l.mode, PlayerFilterMode::Set);
        assert!(l.selection.white[0] && l.selection.black[0] && !l.selection.white[1]);
        let l = parse_link("-F").expect("refine alone");
        assert_eq!(l.mode, PlayerFilterMode::Refine);
        assert_eq!(l.selection, GameSelection::default());
        assert_eq!(
            parse_link("-oaA").map(|l| l.mode),
            Some(PlayerFilterMode::Opponent)
        );
        assert!(parse_link("-fx").is_none(), "unknown letter");
        assert!(parse_link("-g").is_none());
        assert!(parse_link("fw").is_none());
        assert!(parse_link("").is_none());
    }
}
