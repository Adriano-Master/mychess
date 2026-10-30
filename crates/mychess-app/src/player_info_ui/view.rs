//! The snapshot of the player information window, free of `mychess-db`
//! types.

use std::sync::Arc;

use mychess_core::PartialDate;
use mychess_db::player_info::{OpeningShare, PlayerInfo, SpellingHeader, Split, Tally};
use mychess_db::spelling::photo::PhotoKind;

/// The format of a photo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PhotoFormat {
    /// GIF.
    Gif,
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
}

impl PhotoFormat {
    pub(crate) fn of(kind: PhotoKind) -> PhotoFormat {
        match kind {
            PhotoKind::Gif => PhotoFormat::Gif,
            PhotoKind::Png => PhotoFormat::Png,
            PhotoKind::Jpeg => PhotoFormat::Jpeg,
        }
    }
}

/// The photo of a player: the bytes of the image file. This build of the
/// program does not decode images (the `image` feature of the toolkit is not
/// enabled, D3464): the front-end keeps the bytes and shows no photo until
/// it can.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerPhoto {
    /// The image file.
    pub bytes: Arc<[u8]>,
    /// Its format.
    pub format: PhotoFormat,
}

/// Wins, draws, losses and games without result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TallyView {
    /// Wins.
    pub wins: u32,
    /// Draws.
    pub draws: u32,
    /// Losses.
    pub losses: u32,
    /// Games without a result.
    pub unfinished: u32,
    /// All the games.
    pub games: u32,
    /// The score in hundredths of a percent (`8750` = `87.50%`).
    pub percent_hundredths: u32,
    /// The points as text (`3.5`).
    pub points: String,
}

impl TallyView {
    fn of(t: &Tally) -> TallyView {
        TallyView {
            wins: t.wins,
            draws: t.draws,
            losses: t.losses,
            unfinished: t.unfinished,
            games: t.games(),
            percent_hundredths: t.percent_hundredths(),
            points: t.points_text(),
        }
    }
}

/// The Total, White and Black lines of one section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SplitView {
    /// Both colours.
    pub total: TallyView,
    /// With White.
    pub white: TallyView,
    /// With Black.
    pub black: TallyView,
}

impl SplitView {
    fn of(s: &Split) -> SplitView {
        SplitView {
            total: TallyView::of(&s.total),
            white: TallyView::of(&s.white),
            black: TallyView::of(&s.black),
        }
    }
}

/// An opening group of the list "most common openings".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningView {
    /// The group (`B9` is B90 to B99).
    pub label: String,
    /// The games.
    pub games: u32,
    /// The score in percent.
    pub score_percent: u32,
}

impl OpeningView {
    fn of(o: &OpeningShare) -> OpeningView {
        OpeningView {
            label: o.label(),
            games: o.games,
            score_percent: o.score_percent,
        }
    }
}

/// What the spelling file says about the player (section 3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoHeader {
    /// The codes of the titles as written in the file (`gm`, `wim`).
    pub title_codes: Vec<String>,
    /// The names of the titles (`Grandmaster`); the code when unknown.
    pub title_names: Vec<String>,
    /// The names of the countries.
    pub country_names: Vec<String>,
    /// The peak rating; 0 when unknown.
    pub peak: u16,
    /// The peak is an estimate (the file marks it with `*`).
    pub peak_is_estimate: bool,
    /// The birth and death as written (`1904.??.??--1979.08.15`).
    pub born: String,
    /// The notes of the biography.
    pub bio: Vec<String>,
    /// The address of the FIDE profile, when a note has `FIDEID`.
    pub fide_url: Option<String>,
    /// The data come from a name that only starts like the player's
    /// (D3319): the window warns that they are approximate.
    pub approximate: bool,
    /// The names that match, when the match is not exact (at most 10).
    pub candidates: Vec<String>,
}

impl InfoHeader {
    pub(crate) fn of(
        h: &SpellingHeader,
        candidates: Vec<String>,
        fide_url: Option<String>,
    ) -> InfoHeader {
        InfoHeader {
            title_codes: h.title_codes.clone(),
            title_names: h.title_names.clone(),
            country_names: h.country_names.clone(),
            peak: h.peak,
            peak_is_estimate: h.peak_is_estimate,
            born: h.born.clone(),
            bio: h.bio.clone(),
            fide_url,
            approximate: !h.exact,
            candidates,
        }
    }
}

/// The statistics from the database (section 3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoStats {
    /// The games of the player.
    pub games: u32,
    /// The date of the first game.
    pub first: PartialDate,
    /// The date of the last game (shown only when `last_shown`).
    pub last: PartialDate,
    /// The last date is after the first one.
    pub last_shown: bool,
    /// The games of the player in the filter, when it is not the whole
    /// database ("Filter: M games").
    pub in_filter: Option<u32>,
    /// All the games.
    pub all: SplitView,
    /// The games of the filter, when the filter is not the whole database.
    pub filter: Option<SplitView>,
    /// The games against the opponent of the game on screen.
    pub versus: Option<(String, SplitView)>,
    /// The most played openings with White and with Black.
    pub openings: [Vec<OpeningView>; 2],
    /// The rating history as `(year, month, Elo)`, the changes only.
    pub ratings: Vec<(u16, u8, u16)>,
}

impl InfoStats {
    pub(crate) fn of(info: &PlayerInfo) -> InfoStats {
        let games = info.all.total.games();
        InfoStats {
            games,
            first: info.first,
            last: info.last,
            last_shown: info.last > info.first,
            in_filter: info
                .filter
                .as_ref()
                .filter(|_| !info.filter_is_everything)
                .map(|f| f.total.games()),
            all: SplitView::of(&info.all),
            filter: info
                .filter
                .as_ref()
                .filter(|_| !info.filter_is_everything)
                .map(SplitView::of),
            versus: info
                .versus
                .as_ref()
                .map(|(name, split)| (name.clone(), SplitView::of(split))),
            openings: [
                info.openings[0].iter().map(OpeningView::of).collect(),
                info.openings[1].iter().map(OpeningView::of).collect(),
            ],
            ratings: info.ratings.clone(),
        }
    }
}

/// The player information window.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerInfoView {
    /// The player.
    pub name: String,
    /// The name is in the database.
    pub found: bool,
    /// What the spelling file says.
    pub header: Option<InfoHeader>,
    /// The statistics; `None` while the first answer is awaited and for a
    /// player who is not in the database.
    pub stats: Option<InfoStats>,
    /// The photo, when the library has one.
    pub photo: Option<PlayerPhoto>,
    /// The figures are being worked out: games done and total.
    pub progress: Option<(u64, u64)>,
    /// What is shown is from before the last change (the new answer is on
    /// its way).
    pub stale: bool,
    /// The last players looked up, newest first.
    pub history: Vec<String>,
    /// The opponent of the game on screen, when the player has a game there.
    pub opponent: Option<String>,
    /// A spelling file is loaded.
    pub spelling_loaded: bool,
    /// There is no database.
    pub no_base: bool,
}
