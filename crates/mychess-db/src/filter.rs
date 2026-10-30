// Derived from Scid vs PC 4.27, src/filter.cpp, src/filter.h and the
// `sc_filter` commands of src/tkscid.cpp. Copyright (c) 1999-2002 Shane
// Hudson; Scid vs PC contributors. Scid is distributed under the GNU General
// Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The filter: one byte per game (`docs/specs/m3-3-filtro-busca-importacao.md`,
//! decisions D324 to D327 and D343).
//!
//! `0` means the game is out; `v > 0` means it is in and should be shown at
//! ply `v - 1` when loaded (`1` = start position, `255` = last move). The
//! number of games that are in is kept up to date, and per-block counts make
//! rank and select sublinear (D326).
//!
//! Game numbers in this API run from `1` to [`Filter::len`], like the numbers
//! shown to the user; internally the bytes are indexed from 0.

use std::fmt::Write as _;

use crate::progress::{
    CHECK_INTERVAL, CancelToken, NoProgress, ProgressSink, SEARCH_CHECK_EVERY, Ticker,
};
use crate::si4::{NameBase, NameKind, bytes_to_string};
use crate::source::GameSource;

/// Games per block of the count index (D326).
const BLOCK: usize = 4096;

/// The ply value that means "last move" (`Filter::end`).
pub const FILTER_LAST_MOVE: u8 = 255;

/// How a search combines its result with the current filter.
///
/// Text forms (`.sso` files and scripts): `a`/`0` = [`And`](FilterOp::And),
/// `o`/`1` = [`Or`](FilterOp::Or), `r`/`2` and anything else =
/// [`Reset`](FilterOp::Reset) (D327).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FilterOp {
    /// Restrict the filter: only games already in can stay.
    And,
    /// Add to the filter: games already in are kept as they are.
    Or,
    /// Start from all games (ply 1) and search them all.
    #[default]
    Reset,
}

impl FilterOp {
    /// Reads the operation from its text, looking only at the first
    /// character like Scid's `strGetFilterOp`; unknown text is `Reset`.
    pub fn from_scid_str(text: &str) -> FilterOp {
        match text.chars().next() {
            Some('A' | 'a' | '0') => FilterOp::And,
            Some('O' | 'o' | '1') => FilterOp::Or,
            _ => FilterOp::Reset,
        }
    }
}

/// A filter over the games of a database. See the [module](self)
/// documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    data: Vec<u8>,
    /// Number of non-zero entries of each block of [`BLOCK`] games.
    blocks: Vec<u32>,
    count: u32,
    /// Number of games; `data.len()` always fits (`new` takes a `u32`,
    /// `append` refuses to pass `u32::MAX`).
    len: u32,
}

impl Filter {
    /// A filter of `len` games, all in with ply value 1.
    pub fn new(len: u32) -> Filter {
        let mut f = Filter {
            data: vec![1; len as usize],
            blocks: Vec::new(),
            count: 0,
            len,
        };
        f.recount();
        f
    }

    /// The number of games the filter covers.
    pub fn len(&self) -> u32 {
        self.len
    }

    /// Whether the filter covers no games.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The number of games that are in the filter.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// The raw value of game `game` (`1..=len()`): 0 = out, otherwise the
    /// ply to show plus one. Out of range gives 0.
    pub fn get(&self, game: u32) -> u8 {
        index(game)
            .and_then(|i| self.data.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// Whether game `game` is in the filter.
    pub fn contains(&self, game: u32) -> bool {
        self.get(game) != 0
    }

    /// The ply to show when game `game` is loaded: `value - 1`, or 0 for a
    /// game that is out. `255` stands for "last move" ([`FILTER_LAST_MOVE`]).
    pub fn ply(&self, game: u32) -> u8 {
        self.get(game).saturating_sub(1)
    }

    /// Sets the value of game `game`, keeping the counts. Out of range is
    /// ignored; returns whether the game exists.
    pub fn set(&mut self, game: u32, value: u8) -> bool {
        let Some(i) = index(game).filter(|i| *i < self.data.len()) else {
            return false;
        };
        let Some(slot) = self.data.get_mut(i) else {
            return false;
        };
        let was_in = *slot != 0;
        *slot = value;
        let now_in = value != 0;
        if was_in != now_in {
            if let Some(block) = self.blocks.get_mut(i / BLOCK) {
                if now_in {
                    *block += 1;
                } else {
                    *block -= 1;
                }
            }
            if now_in {
                self.count += 1;
            } else {
                self.count -= 1;
            }
        }
        true
    }

    /// Sets every game to `value` (`sc_filter reset` is `fill(1)`).
    pub fn fill(&mut self, value: u8) {
        self.data.fill(value);
        self.recount();
    }

    /// Adds a game at the end with `value` (D361: a new game enters with 1).
    /// Returns `false`, changing nothing, if the filter is at `u32::MAX`.
    pub fn append(&mut self, value: u8) -> bool {
        if self.data.len() >= u32::MAX as usize {
            return false;
        }
        let i = self.data.len();
        self.data.push(value);
        self.len += 1;
        if i / BLOCK >= self.blocks.len() {
            self.blocks.push(0);
        }
        if value != 0 {
            if let Some(block) = self.blocks.get_mut(i / BLOCK) {
                *block += 1;
            }
            self.count += 1;
        }
        true
    }

    /// Swaps in and out. Everything that was in goes out; everything that
    /// was out comes in with value 1, so the plies are lost (rule 5).
    pub fn negate(&mut self) {
        for v in &mut self.data {
            *v = u8::from(*v == 0);
        }
        self.recount();
    }

    /// Makes every game that is in show its last move (value 255).
    pub fn end(&mut self) {
        for v in self.data.iter_mut().filter(|v| **v != 0) {
            *v = FILTER_LAST_MOVE;
        }
        // The counts do not change.
    }

    /// Removes games `from..=to` from the filter (`sc_filter remove`). The
    /// range is limited to `1..=len()`; an empty range does nothing.
    pub fn remove_range(&mut self, from: u32, to: u32) {
        let from = from.max(1);
        let to = to.min(self.len());
        if from > to {
            return;
        }
        let (Some(a), Some(b)) = (index(from), index(to)) else {
            return;
        };
        if let Some(range) = self.data.get_mut(a..=b) {
            range.fill(0);
        }
        self.recount();
    }

    /// The number of the first game in the filter.
    pub fn first(&self) -> Option<u32> {
        self.nth(1)
    }

    /// The number of the last game in the filter.
    pub fn last(&self) -> Option<u32> {
        self.prev_before(self.len().saturating_add(1))
    }

    /// The number of the `n`-th game in the filter (`n` from 1, Scid's
    /// `sc_filter index`); `None` if `n` is 0 or above [`Filter::count`].
    pub fn nth(&self, n: u32) -> Option<u32> {
        if n == 0 || n > self.count {
            return None;
        }
        let mut left = n;
        for (b, &c) in self.blocks.iter().enumerate() {
            if left > c {
                left -= c;
                continue;
            }
            let start = b * BLOCK;
            let end = (start + BLOCK).min(self.data.len());
            let block = self.data.get(start..end)?;
            let mut seen = 0;
            for (k, &v) in block.iter().enumerate() {
                if v != 0 {
                    seen += 1;
                    if seen == left {
                        return u32::try_from(start + k + 1).ok();
                    }
                }
            }
            return None;
        }
        None
    }

    /// The number of games in the filter whose number is at most `game`:
    /// the position of the game in the filtered list (Scid's
    /// `sc_filter locate`). Zero for game 0 and, as in Scid, for a `game`
    /// beyond the end.
    pub fn rank(&self, game: u32) -> u32 {
        if game == 0 || game > self.len() {
            return 0;
        }
        let upto = game as usize; // number of games to look at
        let full = upto / BLOCK;
        let mut total: u32 = self.blocks.iter().take(full).sum();
        if let Some(rest) = self.data.get(full * BLOCK..upto) {
            total += count_nonzero(rest);
        }
        total
    }

    /// The number of the next game after `game` that is in the filter.
    /// `game` may be 0 ("before the first") or beyond the end.
    pub fn next_after(&self, game: u32) -> Option<u32> {
        let mut i = game as usize; // 0-based index of the game after `game`
        while i < self.data.len() {
            if i.is_multiple_of(BLOCK) && self.blocks.get(i / BLOCK) == Some(&0) {
                i += BLOCK;
                continue;
            }
            if self.data.get(i).is_some_and(|v| *v != 0) {
                return u32::try_from(i + 1).ok();
            }
            i += 1;
        }
        None
    }

    /// The number of the closest game before `game` that is in the filter.
    pub fn prev_before(&self, game: u32) -> Option<u32> {
        // `i` counts down from the 1-based number of the game before `game`.
        let mut i = (game as usize).saturating_sub(1).min(self.data.len());
        while i > 0 {
            if i.is_multiple_of(BLOCK) && self.blocks.get(i / BLOCK - 1) == Some(&0) {
                i -= BLOCK;
                continue;
            }
            if self.data.get(i - 1).is_some_and(|v| *v != 0) {
                return u32::try_from(i).ok();
            }
            i -= 1;
        }
        None
    }

    /// Iterates over the numbers of the games that are in the filter, in
    /// ascending order. Use [`Filter::get`] for their values.
    pub fn iter_games(&self) -> impl Iterator<Item = u32> + '_ {
        self.data
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0)
            .filter_map(|(i, _)| u32::try_from(i + 1).ok())
    }

    /// The raw values, one per game.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Finds the next game after filter position `from` (0 = before the
    /// first) whose White, Black, Event, Site or date text (`yyyy.mm.dd`)
    /// contains `text`; round is not examined. Returns its position in the
    /// filter (as [`Filter::rank`]), or `None`.
    ///
    /// With `ignore_case` the comparison folds case; otherwise it is exact.
    /// Spaces are not ignored (Scid's `strContains`/`strCaseContains`).
    /// Unlike Scid, position 0 examines game 1 (D343).
    ///
    /// An index entry that cannot be read is treated as "does not match" and
    /// is not reported; use [`Filter::textfind_with`] to learn about them.
    pub fn textfind<D: GameSource + ?Sized>(
        &self,
        db: &D,
        text: &str,
        ignore_case: bool,
        from: u32,
    ) -> Option<u32> {
        self.textfind_with(
            db,
            text,
            ignore_case,
            from,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .position
    }

    /// [`Filter::textfind`] with progress and cancellation (D329, D388).
    ///
    /// Progress counts the games examined out of [`Filter::count`]; both
    /// progress and `cancel` are looked at with the same thresholds as the
    /// header search. Unreadable entries are listed in the report.
    pub fn textfind_with<D: GameSource + ?Sized>(
        &self,
        db: &D,
        text: &str,
        ignore_case: bool,
        from: u32,
        progress: &mut dyn ProgressSink,
        cancel: &CancelToken,
    ) -> TextFindReport {
        let mut report = TextFindReport::default();
        let mut game = match self.nth(from) {
            Some(g) => g,
            None if from == 0 => 0,
            None => return report,
        };
        let mut matcher = TextMatcher::new(text, ignore_case);
        let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
        let total = u64::from(self.count);
        let mut done: u64 = 0;
        while let Some(next) = self.next_after(game) {
            if ticker.tick() {
                progress.progress(done, total);
                if cancel.is_cancelled() {
                    report.interrupted = true;
                    return report;
                }
            }
            done += 1;
            match matcher.matches(db, next) {
                Some(true) => {
                    report.position = Some(self.rank(next));
                    return report;
                }
                Some(false) => {}
                None => note_unreadable(&mut report.unreadable, &mut report.unreadable_total, next),
            }
            game = next;
        }
        report
    }

    /// Removes from the filter the games that do **not** contain `text` in
    /// those fields (see [`Filter::textfind`]); the ply of the games that
    /// stay is kept. Returns how many were removed.
    ///
    /// An index entry that cannot be read is treated as "does not match" (the
    /// game is removed) and is not reported; use [`Filter::textfilter_with`]
    /// to learn about them.
    pub fn textfilter<D: GameSource + ?Sized>(
        &mut self,
        db: &D,
        text: &str,
        ignore_case: bool,
    ) -> u32 {
        self.textfilter_with(db, text, ignore_case, &mut NoProgress, &CancelToken::new())
            .removed
    }

    /// [`Filter::textfilter`] with progress and cancellation (D329, D388).
    ///
    /// The work is done on a copy that replaces the filter only at the end, so
    /// when `cancel` is set the filter stays as it was and the report has
    /// `interrupted = true` (and `removed = 0`).
    pub fn textfilter_with<D: GameSource + ?Sized>(
        &mut self,
        db: &D,
        text: &str,
        ignore_case: bool,
        progress: &mut dyn ProgressSink,
        cancel: &CancelToken,
    ) -> TextFilterReport {
        let mut report = TextFilterReport::default();
        if cancel.is_cancelled() {
            report.interrupted = true;
            return report;
        }
        let mut matcher = TextMatcher::new(text, ignore_case);
        let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
        let total = u64::from(self.count);
        let mut done: u64 = 0;
        let mut work = self.clone();
        let mut removed = 0u32;
        let mut game = 0;
        while let Some(next) = self.next_after(game) {
            game = next;
            if ticker.tick() {
                progress.progress(done, total);
                if cancel.is_cancelled() {
                    // Cancelled: the filter is intact, so nothing was removed.
                    report.interrupted = true;
                    report.removed = 0;
                    return report;
                }
            }
            done += 1;
            match matcher.matches(db, next) {
                Some(true) => {}
                other => {
                    if other.is_none() {
                        note_unreadable(&mut report.unreadable, &mut report.unreadable_total, next);
                    }
                    work.set(next, 0);
                    removed += 1;
                }
            }
        }
        progress.progress(total, total);
        *self = work;
        report.removed = removed;
        report
    }

    fn recount(&mut self) {
        self.blocks = self.data.chunks(BLOCK).map(count_nonzero).collect();
        self.count = self.blocks.iter().sum();
    }
}

fn index(game: u32) -> Option<usize> {
    usize::try_from(game.checked_sub(1)?).ok()
}

fn count_nonzero(slice: &[u8]) -> u32 {
    // A block has at most BLOCK entries, so the count fits.
    u32::try_from(slice.iter().filter(|v| **v != 0).count()).unwrap_or(u32::MAX)
}

/// The result of [`Filter::textfind_with`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextFindReport {
    /// Position of the game found in the filter (as [`Filter::rank`]).
    pub position: Option<u32>,
    /// The search was cancelled before it finished.
    pub interrupted: bool,
    /// Games whose index entry could not be read (they do not match); at
    /// most [`MAX_UNREADABLE`] numbers are kept.
    pub unreadable: Vec<u32>,
    /// How many entries could not be read (all of them, not only the numbers kept).
    pub unreadable_total: u64,
}

/// The result of [`Filter::textfilter_with`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextFilterReport {
    /// How many games were removed (0 if interrupted).
    pub removed: u32,
    /// The operation was cancelled; the filter is unchanged.
    pub interrupted: bool,
    /// Games whose index entry could not be read (they were removed); at most
    /// [`MAX_UNREADABLE`] numbers are kept.
    pub unreadable: Vec<u32>,
    /// How many entries could not be read (all of them, not only the numbers kept).
    pub unreadable_total: u64,
}

/// The most unreadable game numbers a text report keeps.
pub const MAX_UNREADABLE: usize = 1000;

/// Records an unreadable game: counted always, listed up to the cap.
fn note_unreadable(list: &mut Vec<u32>, total: &mut u64, n: u32) {
    *total += 1;
    if list.len() < MAX_UNREADABLE {
        list.push(n);
    }
}

/// Answers "does game `n` contain the text?" with one cache per name kind,
/// so each distinct name is examined once; only the date is tested per game,
/// in a reused buffer.
///
/// Public so that a caller can run the search in chunks, releasing a lock on
/// the database between them (the list of the application does, slice 4c,
/// D570); [`Filter::textfind_with`] and [`Filter::textfilter_with`] use the
/// same matcher. Keep one matcher for the whole search: the caches fill up.
pub struct TextMatcher {
    needle: String,
    ignore_case: bool,
    /// 0 = not computed, 1 = no, 2 = yes; by name ID.
    players: Vec<u8>,
    events: Vec<u8>,
    sites: Vec<u8>,
    date: String,
}

impl TextMatcher {
    /// A matcher for `text` (compared without regard to case when
    /// `ignore_case`).
    pub fn new(text: &str, ignore_case: bool) -> TextMatcher {
        TextMatcher {
            needle: if ignore_case {
                crate::search::fold_case(text)
            } else {
                text.to_owned()
            },
            ignore_case,
            players: Vec::new(),
            events: Vec::new(),
            sites: Vec::new(),
            date: String::new(),
        }
    }

    fn found_in(&self, hay: &str) -> bool {
        if self.ignore_case {
            crate::search::fold_case(hay).contains(&self.needle)
        } else {
            hay.contains(&self.needle)
        }
    }

    fn name_has(&self, names: &NameBase, kind: NameKind, id: u32) -> bool {
        let name = names.get(kind, id).map(bytes_to_string);
        self.found_in(name.as_deref().unwrap_or(""))
    }

    fn cache_for(&mut self, kind: NameKind) -> &mut Vec<u8> {
        match kind {
            NameKind::Player => &mut self.players,
            NameKind::Event => &mut self.events,
            _ => &mut self.sites,
        }
    }

    fn cached(&mut self, names: &NameBase, kind: NameKind, id: u32) -> bool {
        let n = usize::try_from(names.num_names(kind)).unwrap_or(0);
        let cache = self.cache_for(kind);
        if cache.is_empty() {
            *cache = vec![0; n];
        }
        let slot = usize::try_from(id).ok();
        match slot.and_then(|i| cache.get(i).copied()) {
            Some(2) => true,
            Some(1) => false,
            _ => {
                let ok = self.name_has(names, kind, id);
                if let Some(s) = slot.and_then(|i| self.cache_for(kind).get_mut(i)) {
                    *s = if ok { 2 } else { 1 };
                }
                ok
            }
        }
    }

    /// Whether White, Black, Event, Site or the date of game `n` contains
    /// the text: `Some(matches)`, or `None` when the entry cannot be read.
    pub fn matches<D: GameSource + ?Sized>(&mut self, db: &D, n: u32) -> Option<bool> {
        let entry = db.entry(n).ok()?;
        let names = db.names();
        if self.cached(names, NameKind::Player, entry.white_id)
            || self.cached(names, NameKind::Player, entry.black_id)
            || self.cached(names, NameKind::Event, entry.event_id)
        {
            return Some(true);
        }
        // The date holds only digits, `.` and `?`, which case folding keeps.
        self.date.clear();
        // Writing to a String cannot fail.
        let _ = write!(self.date, "{}", entry.date);
        Some(self.date.contains(&self.needle) || self.cached(names, NameKind::Site, entry.site_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_public_matcher_agrees_with_textfind() {
        // The behaviour on a database is covered through `textfind` and
        // `textfilter` (same matcher) and by the chunked search of the
        // application; here the folding of the needle.
        let m = TextMatcher::new("Abc", true);
        assert!(m.found_in("xxABCxx"));
        assert!(!m.found_in("xx"));
        let exact = TextMatcher::new("Abc", false);
        assert!(!exact.found_in("xxABCxx"));
    }

    #[test]
    fn new_filter_has_every_game_in_with_ply_one() {
        let f = Filter::new(10);
        assert_eq!((f.len(), f.count()), (10, 10));
        assert!((1..=10).all(|g| f.get(g) == 1));
        assert_eq!(f.get(0), 0);
        assert_eq!(f.get(11), 0);
    }

    #[test]
    fn set_keeps_the_count() {
        let mut f = Filter::new(5);
        assert!(f.set(2, 5));
        assert_eq!(f.count(), 5);
        assert!(f.set(2, 0));
        assert_eq!(f.count(), 4);
        assert!(f.set(2, 7));
        assert_eq!(f.count(), 5);
        assert!(!f.set(6, 1));
        assert!(!f.set(0, 1));
        assert_eq!(f.count(), 5);
    }

    #[test]
    fn fill_negate_and_end() {
        let mut f = Filter::new(6);
        f.fill(0);
        assert_eq!(f.count(), 0);
        f.set(2, 9);
        f.set(5, 3);
        f.negate();
        assert_eq!(f.count(), 4);
        assert_eq!(f.get(2), 0);
        assert_eq!(f.get(1), 1);
        f.end();
        assert_eq!(f.get(1), 255);
        assert_eq!(f.get(2), 0);
        assert_eq!(f.count(), 4);
        assert_eq!(f.ply(1), 254);
    }

    #[test]
    fn remove_range_is_clamped() {
        let mut f = Filter::new(10);
        f.remove_range(0, 2);
        assert_eq!(f.count(), 8);
        f.remove_range(9, 1000);
        assert_eq!(f.count(), 6);
        f.remove_range(5, 3);
        f.remove_range(50, 60);
        assert_eq!(f.count(), 6);
    }

    #[test]
    fn navigation_follows_the_scid_rules() {
        let mut f = Filter::new(10);
        f.fill(0);
        for g in [3, 4, 9] {
            f.set(g, 1);
        }
        assert_eq!((f.first(), f.last()), (Some(3), Some(9)));
        assert_eq!(f.nth(1), Some(3));
        assert_eq!(f.nth(3), Some(9));
        assert_eq!((f.nth(0), f.nth(4)), (None, None));
        assert_eq!(f.rank(2), 0);
        assert_eq!(f.rank(4), 2);
        assert_eq!(f.rank(10), 3);
        assert_eq!(f.rank(11), 0);
        assert_eq!(f.next_after(0), Some(3));
        assert_eq!(f.next_after(3), Some(4));
        assert_eq!(f.next_after(9), None);
        assert_eq!(f.prev_before(3), None);
        assert_eq!(f.prev_before(4), Some(3));
        assert_eq!(f.prev_before(100), Some(9));
        assert_eq!(f.iter_games().collect::<Vec<_>>(), [3, 4, 9]);
    }

    #[test]
    fn empty_filter_has_no_first_or_last() {
        let f = Filter::new(0);
        assert!(f.is_empty());
        assert_eq!((f.first(), f.last(), f.next_after(0)), (None, None, None));
    }

    #[test]
    fn append_grows_across_blocks() {
        let mut f = Filter::new(0);
        for _ in 0..(BLOCK * 2 + 10) {
            assert!(f.append(1));
        }
        assert!(f.append(0));
        assert_eq!(f.count() as usize, BLOCK * 2 + 10);
        assert_eq!(f.len() as usize, BLOCK * 2 + 11);
        assert_eq!(f.last(), Some(u32::try_from(BLOCK * 2 + 10).unwrap_or(0)));
    }

    #[test]
    fn rank_and_nth_agree_across_blocks() {
        let n = BLOCK * 3 + 17;
        let mut f = Filter::new(u32::try_from(n).unwrap_or(0));
        f.fill(0);
        let games: Vec<u32> = (1..=u32::try_from(n).unwrap_or(0))
            .filter(|g| g % 7 == 0 || *g == 4096 || *g == 4097)
            .collect();
        for &g in &games {
            f.set(g, 1);
        }
        for (k, &g) in games.iter().enumerate() {
            let pos = u32::try_from(k + 1).unwrap_or(0);
            assert_eq!(f.nth(pos), Some(g));
            assert_eq!(f.rank(g), pos);
        }
        assert_eq!(f.next_after(8200), f.iter_games().find(|g| *g > 8200));
        assert_eq!(
            f.prev_before(12000),
            f.iter_games().filter(|g| *g < 12000).last()
        );
    }

    #[test]
    fn skipping_empty_blocks_works_in_both_directions() {
        let n = u32::try_from(BLOCK * 4).unwrap_or(0);
        let mut f = Filter::new(n);
        f.fill(0);
        f.set(1, 1);
        f.set(n, 1);
        assert_eq!(f.next_after(1), Some(n));
        assert_eq!(f.prev_before(n), Some(1));
    }

    #[test]
    fn len_follows_append() {
        let mut f = Filter::new(2);
        assert!(f.append(1));
        assert_eq!(f.len(), 3);
    }

    #[test]
    fn unreadable_list_is_capped_but_counted() {
        let mut list = Vec::new();
        let mut total = 0;
        for n in 1..=u32::try_from(MAX_UNREADABLE).unwrap_or(0) + 5 {
            note_unreadable(&mut list, &mut total, n);
        }
        assert_eq!(list.len(), MAX_UNREADABLE);
        assert_eq!(total, MAX_UNREADABLE as u64 + 5);
        assert_eq!(list.first(), Some(&1));
    }

    #[test]
    fn filter_op_reads_the_first_character() {
        assert_eq!(FilterOp::from_scid_str("and"), FilterOp::And);
        assert_eq!(FilterOp::from_scid_str("0"), FilterOp::And);
        assert_eq!(FilterOp::from_scid_str("Or"), FilterOp::Or);
        assert_eq!(FilterOp::from_scid_str("1"), FilterOp::Or);
        assert_eq!(FilterOp::from_scid_str("2"), FilterOp::Reset);
        assert_eq!(FilterOp::from_scid_str("x"), FilterOp::Reset);
        assert_eq!(FilterOp::from_scid_str(""), FilterOp::Reset);
    }
}
