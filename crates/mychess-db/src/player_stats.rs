//! Highest rating of each player of a database (decision D332).
//!
//! Scid keeps, per player name, the highest Elo seen in any game of the
//! database (`NameBase::AddElo`, filled by `recalcFlagCounts`,
//! `tkscid.cpp:226-243`). The header search uses it when a game has no Elo of
//! its own (rule 26).

use crate::progress::CancelToken;
use crate::si4::IndexEntry;
use crate::source::GameSource;

/// How many entries pass between looks at the cancellation token.
const CANCEL_CHECK_EVERY: u32 = 4096;

/// The highest Elo of every player name of a database, indexed by name ID.
///
/// Compute it once when the database opens with
/// [`PlayerStats::from_database`], then keep it current with
/// [`PlayerStats::add_game`] when a game is added and
/// [`PlayerStats::recompute_players`] when one is replaced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerStats {
    max_elo: Vec<u16>,
}

impl PlayerStats {
    /// Empty statistics: every player has Elo 0.
    pub fn new() -> PlayerStats {
        PlayerStats::default()
    }

    /// Scans the index of `db`. Entries that cannot be read are skipped.
    pub fn from_database<D: GameSource + ?Sized>(db: &D) -> PlayerStats {
        // A token nobody cancels: never `None`.
        PlayerStats::from_database_cancellable(db, &CancelToken::new()).unwrap_or_default()
    }

    /// Like [`PlayerStats::from_database`], but looks at `cancel` every
    /// 4096 entries and gives up with `None` once it is
    /// cancelled, so the caller can rewrite the files soon after.
    pub fn from_database_cancellable<D: GameSource + ?Sized>(
        db: &D,
        cancel: &CancelToken,
    ) -> Option<PlayerStats> {
        let mut stats = PlayerStats {
            max_elo: vec![0; player_count(db)],
        };
        for n in 1..=db.len() {
            if (n - 1) % CANCEL_CHECK_EVERY == 0 && cancel.is_cancelled() {
                return None;
            }
            if let Ok(entry) = db.entry(n) {
                stats.add_game(&entry);
            }
        }
        Some(stats)
    }

    /// The highest Elo seen for the player with name ID `id`; 0 if none.
    pub fn max_elo(&self, id: u32) -> u16 {
        usize::try_from(id)
            .ok()
            .and_then(|i| self.max_elo.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// Raises the Elo of player `id` to at least `elo` (Scid's `AddElo`).
    /// Also the way to apply the peak rating from a spelling file.
    pub fn raise(&mut self, id: u32, elo: u16) {
        if elo == 0 {
            return;
        }
        let Ok(i) = usize::try_from(id) else {
            return;
        };
        if i >= self.max_elo.len() {
            self.max_elo.resize(i + 1, 0);
        }
        if let Some(slot) = self.max_elo.get_mut(i) {
            *slot = (*slot).max(elo);
        }
    }

    /// Takes the ratings of one game: call it for a game that was added.
    pub fn add_game(&mut self, entry: &IndexEntry) {
        self.raise(entry.white_id, entry.white_elo);
        self.raise(entry.black_id, entry.black_elo);
    }

    /// Recomputes the Elo of the given players from every game of `db`.
    /// Call it after a game is replaced, with the players of the old and of
    /// the new game: the highest Elo may have gone down.
    pub fn recompute_players<D: GameSource + ?Sized>(&mut self, db: &D, players: &[u32]) {
        for &id in players {
            if let Some(slot) = usize::try_from(id)
                .ok()
                .and_then(|i| self.max_elo.get_mut(i))
            {
                *slot = 0;
            }
        }
        for entry in (1..=db.len()).filter_map(|n| db.entry(n).ok()) {
            for (id, elo) in [
                (entry.white_id, entry.white_elo),
                (entry.black_id, entry.black_elo),
            ] {
                if players.contains(&id) {
                    self.raise(id, elo);
                }
            }
        }
    }
}

fn player_count<D: GameSource + ?Sized>(db: &D) -> usize {
    usize::try_from(db.names().num_names(crate::si4::NameKind::Player)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raise_keeps_the_maximum_and_grows() {
        let mut s = PlayerStats::new();
        assert_eq!(s.max_elo(3), 0);
        s.raise(3, 2500);
        s.raise(3, 2400);
        s.raise(3, 0);
        s.raise(1, 100);
        assert_eq!((s.max_elo(3), s.max_elo(1), s.max_elo(2)), (2500, 100, 0));
        assert_eq!(s.max_elo(u32::MAX), 0);
    }

    #[test]
    fn a_cancelled_token_stops_the_scan_and_a_free_one_finishes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.pgn");
        std::fs::write(
            &path,
            "[White \"A\"]\n[Black \"B\"]\n[WhiteElo \"2500\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n",
        )
        .unwrap();
        let (db, _) = crate::PgnDatabase::open(
            &path,
            &crate::OpenOptions::default(),
            &mut crate::NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        let token = CancelToken::new();
        let stats = PlayerStats::from_database_cancellable(&db, &token).expect("not cancelled");
        assert_eq!(stats, PlayerStats::from_database(&db));
        token.cancel();
        assert!(PlayerStats::from_database_cancellable(&db, &token).is_none());
    }
}
