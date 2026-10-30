//! Finding a bookmarked game again (spec M3-6, D674; Scid:
//! `sc_game_find`, `src/tkscid.cpp`).

use mychess_core::GameResult;
use mychess_db::GameSource;
use mychess_db::si4::{NameKind, bytes_to_string};

use super::tree::GameBookmark;

/// The game of `source` that best matches `bookmark`: one point for each of
/// white, black, site, round, year and result that is equal, the number
/// stored in the bookmark winning ties. Returns `None` for an empty
/// database. A game that scores 6 at the stored number is returned at once.
pub(crate) fn best_match<S: GameSource + ?Sized>(
    source: &S,
    bookmark: &GameBookmark,
) -> Option<u32> {
    let total = source.len();
    if total == 0 {
        return None;
    }
    let names = source.names();
    let result = GameResult::from_str_lenient(&bookmark.result);
    let score = |number: u32| -> u32 {
        let Ok(entry) = source.entry(number) else {
            return 0;
        };
        let same = |kind, id, wanted: &str| {
            names
                .get(kind, id)
                .is_some_and(|bytes| bytes_to_string(bytes) == wanted)
        };
        u32::from(same(NameKind::Player, entry.white_id, &bookmark.white))
            + u32::from(same(NameKind::Player, entry.black_id, &bookmark.black))
            + u32::from(same(NameKind::Site, entry.site_id, &bookmark.site))
            + u32::from(same(NameKind::Round, entry.round_id, &bookmark.round))
            + u32::from(entry.date.year().unwrap_or(0) == bookmark.year)
            + u32::from(entry.result == result)
    };
    let stored = bookmark.game.checked_sub(1).filter(|n| *n < total);
    if let Some(n) = stored
        && score(n + 1) == 6
    {
        return Some(n + 1);
    }
    let mut best = (0u32, 0u32);
    for number in 1..=total {
        let points = score(number);
        if points > best.1 || (points == best.1 && Some(number - 1) == stored) {
            best = (number, points);
        }
        if points == 6 {
            break;
        }
    }
    Some(best.0.max(1))
}
