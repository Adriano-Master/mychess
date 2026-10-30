//! Changes to the Clipbase in place: the in-memory counterparts of
//! `Si4Writer::apply_name_changes` and `Si4Writer::set_elo_batch` (spec M6-3,
//! D3331, D3461). There is no `.sn4` and no `fsync`; the entries are patched
//! byte by byte exactly as the writer patches the file (offsets of
//! `docs/formatos/si4.md`), so the two paths agree.
//!
//! One limit: the name base of the Clipbase has no way to lower a
//! frequency, so an old name keeps the frequency it had. Nothing in the
//! application reads it (the tables recount from the index).

use std::collections::HashMap;

use mychess_core::{MAX_ELO, PartialDate};
use mychess_db::progress::{CancelToken, ProgressSink};
use mychess_db::si4::{
    ENTRY_LEN, FlagOp, GameFlags, NameChange, NameChangeReport, NameKind, life_span_allows,
};
use mychess_db::spelling::elo::EloChange;

use super::{Clipbase, Undo, hash_of, kind_slot};

/// Entries between two looks at the cancel flag.
const CHECK_EVERY: usize = 4096;
/// Offset of the two rating words inside an entry.
const ELO_OFFSET: usize = 29;
/// Offset of the 20-bit date inside an entry.
const DATE_OFFSET: usize = 25;

/// The five name IDs of an entry: White, Black, event, site, round.
fn read_ids(e: &[u8]) -> Option<[u32; 5]> {
    let b = |at: usize| e.get(at).copied();
    let word = |at: usize| Some(u32::from(u16::from_be_bytes([b(at)?, b(at + 1)?])));
    Some([
        u32::from(b(9)? >> 4) << 16 | word(10)?,
        u32::from(b(9)? & 0xF) << 16 | word(12)?,
        u32::from(b(14)? >> 5) << 16 | word(15)?,
        u32::from((b(14)? >> 2) & 7) << 16 | word(17)?,
        u32::from(b(14)? & 3) << 16 | word(19)?,
    ])
}

/// Writes the IDs back, leaving every other bit as it is.
fn write_ids(e: &mut [u8], ids: [u32; 5]) -> Option<()> {
    let low = |id: u32| ((id & 0xFFFF) as u16).to_be_bytes();
    let high = |id: u32| u8::try_from(id >> 16).unwrap_or(0);
    *e.get_mut(9)? = high(ids[0]) << 4 | (high(ids[1]) & 0xF);
    e.get_mut(10..12)?.copy_from_slice(&low(ids[0]));
    e.get_mut(12..14)?.copy_from_slice(&low(ids[1]));
    *e.get_mut(14)? = high(ids[2]) << 5 | (high(ids[3]) & 7) << 2 | (high(ids[4]) & 3);
    e.get_mut(15..17)?.copy_from_slice(&low(ids[2]));
    e.get_mut(17..19)?.copy_from_slice(&low(ids[3]));
    e.get_mut(19..21)?.copy_from_slice(&low(ids[4]));
    Some(())
}

impl Clipbase {
    /// The ID of an existing name (the base keeps 255 bytes of a name).
    fn find_name(&self, kind: NameKind, name: &[u8]) -> Option<u32> {
        let name = name.get(..255).unwrap_or(name);
        self.lookup[kind_slot(kind)]
            .get(&hash_of(name))?
            .iter()
            .copied()
            .find(|id| self.names.get(kind, *id) == Some(name))
    }

    /// Applies name corrections of one kind in memory (the Clipbase
    /// counterpart of `Si4Writer::apply_name_changes`, rules 48 to 54).
    /// Nothing changes when no correction maps to a different name.
    pub(crate) fn apply_name_changes(
        &mut self,
        kind: NameKind,
        changes: &[NameChange],
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
    ) -> NameChangeReport {
        let mut report = NameChangeReport::default();
        let undo = Undo {
            entries: self.entries.len(),
            records: self.records.len(),
            names: NameKind::ALL.map(|k| self.names.num_names(k)),
        };
        let mut map: HashMap<u32, (u32, PartialDate, PartialDate)> = HashMap::new();
        for change in changes {
            if change.new.is_empty() {
                continue;
            }
            let Some(old) = self.find_name(kind, change.old.as_bytes()) else {
                report.names_not_found = report.names_not_found.saturating_add(1);
                continue;
            };
            let new = if let Some(id) = self.find_name(kind, change.new.as_bytes()) {
                id
            } else if let Ok(id) = self.name_id(kind, change.new.as_bytes()) {
                id
            } else {
                report.skipped_full = report.skipped_full.saturating_add(1);
                continue;
            };
            map.insert(old, (new, change.born, change.died));
        }
        map.retain(|old, (new, _, _)| old != new);
        report.mapped = u32::try_from(map.len()).unwrap_or(u32::MAX);
        if map.is_empty() {
            self.rollback(&undo);
            return report;
        }
        let fields: &[usize] = match kind {
            NameKind::Player => &[0, 1],
            NameKind::Event => &[2],
            NameKind::Site => &[3],
            NameKind::Round => &[4],
        };
        let mut gained: HashMap<u32, u32> = HashMap::new();
        let total = self.entries.len() / ENTRY_LEN;
        #[allow(clippy::chunks_exact_to_as_chunks)] // the length is a runtime slice
        for (index, entry) in self.entries.chunks_exact_mut(ENTRY_LEN).enumerate() {
            if index % CHECK_EVERY == 0 {
                progress.progress(index as u64, total as u64);
                if cancel.is_cancelled() {
                    report.interrupted = true;
                    break;
                }
            }
            let Some(mut ids) = read_ids(entry) else {
                continue;
            };
            let date = entry
                .get(DATE_OFFSET..DATE_OFFSET + 4)
                .and_then(|d| <[u8; 4]>::try_from(d).ok())
                .map_or(0, |d| u32::from_be_bytes(d) & 0x000F_FFFF);
            let mut changed = false;
            for &field in fields {
                let Some(&old) = ids.get(field) else {
                    continue;
                };
                let Some(&(new, born, died)) = map.get(&old) else {
                    continue;
                };
                if kind == NameKind::Player && !life_span_allows(date, born, died) {
                    report.skipped_by_date = report.skipped_by_date.saturating_add(1);
                    continue;
                }
                if let Some(slot) = ids.get_mut(field) {
                    *slot = new;
                }
                *gained.entry(new).or_insert(0) += 1;
                report.games_changed = report.games_changed.saturating_add(1);
                changed = true;
            }
            if changed {
                let _ = write_ids(entry, ids);
            }
        }
        if report.games_changed == 0 {
            // Names created for corrections that touched no game (all
            // skipped by date, or cancelled at once) must not stay.
            self.rollback(&undo);
            progress.progress(total as u64, total as u64);
            return report;
        }
        for (id, count) in gained {
            self.names.add_frequency(kind, id, count);
        }
        progress.progress(total as u64, total as u64);
        report
    }

    /// Sets the ratings of the games of `changes` in memory (the counterpart
    /// of `Si4Writer::set_elo_batch`); returns how many entries changed.
    pub(crate) fn apply_elo_changes(&mut self, changes: &[EloChange]) -> u32 {
        let mut changed = 0u32;
        for change in changes {
            let Some(start) = usize::try_from(change.game.saturating_sub(1))
                .ok()
                .and_then(|i| i.checked_mul(ENTRY_LEN))
                .filter(|_| change.game >= 1)
            else {
                continue;
            };
            let Some(bytes) = self
                .entries
                .get_mut(start + ELO_OFFSET..start + ELO_OFFSET + 4)
            else {
                continue;
            };
            let old: [u8; 4] = match <[u8; 4]>::try_from(&*bytes) {
                Ok(old) => old,
                Err(_) => continue,
            };
            let mut new = old;
            let word = |elo: u16, kind: u8| {
                ((u16::from(kind & 0xF) << 12) | elo.min(MAX_ELO)).to_be_bytes()
            };
            if let Some((elo, kind)) = change.white {
                new[..2].copy_from_slice(&word(elo, kind));
            }
            if let Some((elo, kind)) = change.black {
                new[2..].copy_from_slice(&word(elo, kind));
            }
            if new != old {
                bytes.copy_from_slice(&new);
                changed = changed.saturating_add(1);
            }
        }
        changed
    }

    /// Sets, clears or flips `flag` in the games of `games` in memory (the
    /// counterpart of `Si4Writer::set_flags_batch`, spec M7-1, rule 12): only
    /// bytes 6-8 of an entry are written, and only when the flags change.
    /// Numbers out of range and `0` are ignored; a repeated number counts
    /// once. Returns how many entries changed.
    pub(crate) fn set_flags(&mut self, games: &[u32], flag: GameFlags, op: FlagOp) -> u32 {
        let mut numbers: Vec<u32> = games.iter().copied().filter(|n| *n != 0).collect();
        numbers.sort_unstable();
        numbers.dedup();
        let mut changed = 0u32;
        for number in numbers {
            let Some(start) = usize::try_from(number - 1)
                .ok()
                .and_then(|i| i.checked_mul(ENTRY_LEN))
            else {
                continue;
            };
            let Some(bytes) = self.entries.get_mut(start + 6..start + 9) else {
                continue;
            };
            let [b6, b7, b8] = *bytes else {
                continue;
            };
            let old = GameFlags::from_bits_retain(
                (u32::from(b6 & 0x3F) << 16) | (u32::from(b7) << 8) | u32::from(b8),
            );
            let new = match op {
                FlagOp::Set => old | flag,
                FlagOp::Clear => old & !flag,
                FlagOp::Toggle => old ^ flag,
            };
            if new == old {
                continue;
            }
            let low = u16::try_from(new.bits() & 0xFFFF)
                .unwrap_or(0)
                .to_be_bytes();
            let custom = u8::try_from((new.bits() >> 16) & 0x3F).unwrap_or(0);
            bytes.copy_from_slice(&[(b6 & 0xC0) | custom, low[0], low[1]]);
            changed = changed.saturating_add(1);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::{Game, GameResult};
    use mychess_db::GameSource;
    use mychess_db::progress::NoProgress;

    use super::*;

    fn game(white: &str, black: &str, date: &str) -> Game {
        let mut game = Game::new();
        let tags = game.tags_mut();
        tags.white = white.to_owned();
        tags.black = black.to_owned();
        tags.date = PartialDate::parse_lenient(date);
        tags.result = GameResult::Draw;
        game
    }

    fn clip() -> Clipbase {
        let mut clip = Clipbase::new(10).expect("clipbase");
        clip.push_game(&game("Almeida, B", "Zed", "1960.01.01"))
            .expect("1");
        clip.push_game(&game("Zed", "Almeida, B", "1980.01.01"))
            .expect("2");
        clip.push_game(&game("Other", "Zed", "????.??.??"))
            .expect("3");
        clip
    }

    fn change(old: &str, new: &str, born: &str, died: &str) -> NameChange {
        NameChange {
            old: old.to_owned(),
            new: new.to_owned(),
            born: PartialDate::parse_lenient(born),
            died: PartialDate::parse_lenient(died),
        }
    }

    #[test]
    fn a_correction_replaces_the_name_in_both_colours_and_respects_the_life_span() {
        let mut clip = clip();
        let report = clip.apply_name_changes(
            NameKind::Player,
            &[change(
                "Almeida, B",
                "Almeida, Bruno",
                "1900.??.??",
                "1970.04.??",
            )],
            &CancelToken::new(),
            &mut NoProgress,
        );
        assert_eq!(report.mapped, 1);
        assert_eq!(report.games_changed, 1, "the 1960 game");
        assert_eq!(report.skipped_by_date, 1, "the 1980 game");
        assert_eq!(clip.game(1).expect("g1").tags().white, "Almeida, Bruno");
        assert_eq!(clip.game(2).expect("g2").tags().black, "Almeida, B");
        assert_eq!(clip.len(), 3);
    }

    #[test]
    fn unknown_old_names_are_counted_and_nothing_is_added_without_a_valid_correction() {
        let mut clip = clip();
        let names = clip.names().num_names(NameKind::Player);
        let report = clip.apply_name_changes(
            NameKind::Player,
            &[
                change("Nobody", "Somebody", "", ""),
                change("Zed", "Zed", "", ""),
            ],
            &CancelToken::new(),
            &mut NoProgress,
        );
        assert_eq!(report.names_not_found, 1);
        assert_eq!(report.mapped, 0);
        assert_eq!(
            clip.names().num_names(NameKind::Player),
            names,
            "rolled back"
        );
        assert!(clip.find_name(NameKind::Player, b"Somebody").is_none());
    }

    #[test]
    fn a_new_name_that_exists_is_reused() {
        let mut clip = clip();
        let names = clip.names().num_names(NameKind::Player);
        let report = clip.apply_name_changes(
            NameKind::Player,
            &[change("Other", "Zed", "", "")],
            &CancelToken::new(),
            &mut NoProgress,
        );
        assert_eq!(report.games_changed, 1);
        assert_eq!(clip.names().num_names(NameKind::Player), names);
        assert_eq!(clip.game(3).expect("g3").tags().white, "Zed");
    }

    #[test]
    fn names_created_for_corrections_that_changed_no_game_are_rolled_back() {
        let mut clip = clip();
        let names = clip.names().num_names(NameKind::Player);
        // Both games of "Almeida, B" fall outside the life span.
        let report = clip.apply_name_changes(
            NameKind::Player,
            &[change("Almeida, B", "Almeida, Bruno", "1990.??.??", "")],
            &CancelToken::new(),
            &mut NoProgress,
        );
        assert_eq!(report.games_changed, 0);
        assert_eq!(report.mapped, 1);
        assert_eq!(clip.names().num_names(NameKind::Player), names);
        assert!(
            clip.find_name(NameKind::Player, b"Almeida, Bruno")
                .is_none()
        );
    }

    #[test]
    fn ratings_are_patched_and_only_real_changes_count() {
        let mut clip = clip();
        let change = EloChange {
            game: 2,
            white: Some((2500, 0)),
            black: Some((9999, 0)),
        };
        assert_eq!(clip.apply_elo_changes(&[change]), 1);
        let entry = clip.entry(2).expect("entry");
        assert_eq!((entry.white_elo, entry.black_elo), (2500, 4000));
        assert_eq!(clip.apply_elo_changes(&[change]), 0, "same values");
        let missing = EloChange { game: 99, ..change };
        assert_eq!(clip.apply_elo_changes(&[missing]), 0);
        let zero = EloChange { game: 0, ..change };
        assert_eq!(clip.apply_elo_changes(&[zero]), 0);
    }
}
