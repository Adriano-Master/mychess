// Derived from Scid vs PC 4.27, tcl/windows/tree.tcl (`::tree::mask::feedMask`
// 2487-2539, `fillWithLine` 2400-2461, `fillWithGame`, `fillWithBase`,
// `::tree::parseGame` 1712-1770) and src/tkscid.cpp (`getCleanComment`
// 11114-11125). Copyright (c) 1999-2002 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License (see
// its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Filling a mask from games (decisions D1823 to D1826): from the moves up to
//! a cursor, from a whole game, or from the games of a filter.

use mychess_core::{Color, Game, GameError, Nag, NodeId, Position, Variant, strip_mark_codes};
use thiserror::Error;

use super::matching::{MoveMatcher, mask_san};
use super::model::{MaskInsertError, MaskKey, MaskMove, MaskNag, TreeMask, clean_comment};
use super::ops::find_move;
use crate::filter::Filter;
use crate::progress::{CancelToken, ProgressSink};
use crate::source::GameSource;

/// Which moves of a game are copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FillSide {
    /// Every move.
    #[default]
    All,
    /// Only the moves of White.
    White,
    /// Only the moves of Black.
    Black,
}

impl FillSide {
    fn accepts(self, mover: Color) -> bool {
        match self {
            FillSide::All => true,
            FillSide::White => mover == Color::White,
            FillSide::Black => mover == Color::Black,
        }
    }
}

/// Errors of the filling functions.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FillError {
    /// The game is not standard chess (D1835).
    #[error("masks only hold standard chess")]
    NotStandard,
    /// The node is not part of the game.
    #[error("invalid game node")]
    Node(#[from] GameError),
    /// The filter was made for another number of games.
    #[error("filter has {filter} games but the database has {games}")]
    FilterSize {
        /// The size of the filter.
        filter: u32,
        /// The number of games of the database.
        games: u32,
    },
}

/// What a filling did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FillReport {
    /// Games read and used.
    pub games: u64,
    /// Games skipped (unreadable or not standard chess).
    pub skipped: u64,
    /// Moves visited.
    pub moves: u64,
    /// Whether the mask changed. It is true when something was changed, even
    /// if the run then failed on a game or was cancelled (D1855).
    pub changed: bool,
    /// Whether something was left out because the mask has
    /// [`MAX_POSITIONS`](super::MAX_POSITIONS) positions (D1850).
    pub mask_full: bool,
    /// Whether the run was cancelled (the partial result stays).
    pub cancelled: bool,
}

/// The text of a comment as the mask stores it (`getCleanComment`): without
/// `[%...]` commands, empty when it is only a numeric evaluation.
fn clean_game_comment(text: &str) -> String {
    let stripped = strip_mark_codes(text);
    let (clean, _) = clean_comment(&stripped);
    if is_score(&clean) {
        String::new()
    } else {
        clean
    }
}

/// A lone signed number, possibly in parentheses or brackets (`+0.35`,
/// `(-1.2)`).
fn is_score(text: &str) -> bool {
    let t = text.trim_matches(['(', ')', '[', ']']).trim();
    let Some(rest) = t.strip_prefix(['+', '-']) else {
        return false;
    };
    let mut parts = rest.splitn(2, '.');
    let int = parts.next().unwrap_or("");
    let frac = parts.next();
    !int.is_empty()
        && int.bytes().all(|b| b.is_ascii_digit())
        && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
}

/// Adds `new` to `old` on a new line, unless the lines of `new` are already
/// a block of lines of `old` (D1824 d). `None` when nothing changes.
fn merge_comment(old: &str, new: &str) -> Option<String> {
    if new.is_empty() || old == new {
        return None;
    }
    if old.is_empty() {
        return Some(new.to_owned());
    }
    let old_lines: Vec<&str> = old.lines().map(str::trim).collect();
    let new_lines: Vec<&str> = new.lines().map(str::trim).collect();
    if old_lines
        .windows(new_lines.len())
        .any(|w| w == new_lines.as_slice())
    {
        return None;
    }
    Some(format!("{old}\n{new}"))
}

/// The symbol of a NAG that goes to the comment: its ASCII symbol, else `$n`.
fn nag_symbol(nag: Nag) -> String {
    nag.ascii()
        .map_or_else(|| format!("${}", nag.value()), str::to_owned)
}

/// The NAG that the Scid treats as the diagram marker.
const DIAGRAM_NAG: u8 = 201;

impl TreeMask {
    /// D1823: records one move of a game, played from `before`, with the
    /// first NAG and the comment of the move. Returns whether the mask
    /// changed.
    pub(crate) fn feed_move(
        &mut self,
        before: &Position,
        san: &str,
        nag: Option<Nag>,
        comment: Option<&str>,
    ) -> Result<bool, MaskInsertError> {
        let key = MaskKey::from_position(before);
        let matcher = MoveMatcher::new(Some(before));
        let mut changed = false;
        let entry = self.entry(&key).ok_or(MaskInsertError::Full)?;
        let index = if let Some(i) = find_move(entry, &matcher, san) {
            i
        } else {
            if entry.moves.len() >= super::model::MAX_MOVES_PER_POSITION {
                return Ok(false);
            }
            entry.moves.push(MaskMove::new(san));
            changed = true;
            entry.moves.len() - 1
        };
        let mut text = comment.map(clean_game_comment).unwrap_or_default();
        let mut new_nag = None;
        if let Some(nag) = nag
            && nag.value() != DIAGRAM_NAG
        {
            match MaskNag::from_nag(nag) {
                Some(std_nag) => new_nag = Some(std_nag),
                None if text.is_empty() => text = nag_symbol(nag),
                None => text = format!("{text} ({})", nag_symbol(nag)),
            }
        }
        if let Some(m) = entry.moves.get_mut(index) {
            if let Some(n) = new_nag
                && m.nag.as_ref() != Some(&n)
            {
                m.nag = Some(n);
                changed = true;
            }
            if let Some(merged) = merge_comment(&m.comment, &clean_comment(&text).0) {
                m.comment = clean_comment(&merged).0;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// D1825: adds `text` to the comment of the position `before`.
    fn feed_position_comment(
        &mut self,
        before: &Position,
        text: &str,
    ) -> Result<bool, MaskInsertError> {
        let text = clean_game_comment(text);
        if text.is_empty() {
            return Ok(false);
        }
        let key = MaskKey::from_position(before);
        let entry = self.entry(&key).ok_or(MaskInsertError::Full)?;
        if let Some(merged) = merge_comment(&entry.comment, &text) {
            entry.comment = clean_comment(&merged).0;
            Ok(true)
        } else {
            if entry.moves.is_empty() && entry.comment.is_empty() {
                self.remove(&key);
            }
            Ok(false)
        }
    }
}

fn check_standard(game: &Game) -> Result<(), FillError> {
    if game.variant() == Variant::Standard {
        Ok(())
    } else {
        Err(FillError::NotStandard)
    }
}

fn feed_nodes(
    mask: &mut TreeMask,
    game: &Game,
    nodes: impl IntoIterator<Item = NodeId>,
    side: FillSide,
    report: &mut FillReport,
) -> Result<(), FillError> {
    for id in nodes {
        let Some(parent) = game.parent(id)? else {
            continue;
        };
        let before = game.position_at(parent)?;
        if !side.accepts(before.turn()) {
            continue;
        }
        let mv = game.mv(id)?;
        let Some(san) = mask_san(before, mv) else {
            continue;
        };
        report.moves += 1;
        let nag = game.nags(id)?.first().copied();
        let comment = game.comment(id)?;
        match mask.feed_move(before, &san, nag, comment) {
            Ok(changed) => report.changed |= changed,
            Err(_) => report.mask_full = true,
        }
    }
    Ok(())
}

/// Copies a whole game, main line and variations, into the mask (D1823,
/// D1825): moves, the first NAG of each, comments, and the comment before the
/// first move as the comment of the start position.
///
/// # Errors
///
/// [`FillError::NotStandard`] for Chess960.
pub fn feed_game(
    mask: &mut TreeMask,
    game: &Game,
    side: FillSide,
) -> Result<FillReport, FillError> {
    let mut report = FillReport::default();
    feed_game_into(mask, game, side, &mut report)?;
    report.games = 1;
    Ok(report)
}

/// [`feed_game`] writing into `report` as it goes, so the caller keeps what
/// was done when the game fails halfway (D1855).
fn feed_game_into(
    mask: &mut TreeMask,
    game: &Game,
    side: FillSide,
    report: &mut FillReport,
) -> Result<(), FillError> {
    check_standard(game)?;
    if let Some(text) = game.initial_comment() {
        match mask.feed_position_comment(game.start_position(), text) {
            Ok(changed) => report.changed |= changed,
            Err(_) => report.mask_full = true,
        }
    }
    feed_nodes(mask, game, game.descendants(game.root())?, side, report)
}

/// Copies the moves from the start of the game to `until` (the cursor, which
/// may be inside a variation) into the mask (D1826). The start position
/// itself and the moves after the cursor are not used.
///
/// # Errors
///
/// [`FillError::NotStandard`], and [`FillError::Node`] if `until` is not in
/// the game.
pub fn feed_line(
    mask: &mut TreeMask,
    game: &Game,
    until: NodeId,
    side: FillSide,
) -> Result<FillReport, FillError> {
    check_standard(game)?;
    let mut report = FillReport::default();
    feed_nodes(mask, game, game.path_to(until)?, side, &mut report)?;
    report.games = 1;
    Ok(report)
}

/// Copies every game of `filter` into the mask (D1826), reporting progress
/// (`done`, `total`) after each game. A cancelled run returns what was done
/// with [`FillReport::cancelled`] set. Games that cannot be read or are not
/// standard chess are skipped and counted.
///
/// # Errors
///
/// [`FillError::FilterSize`].
pub fn feed_filter<D: GameSource + ?Sized>(
    mask: &mut TreeMask,
    db: &mut D,
    filter: &Filter,
    side: FillSide,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<FillReport, FillError> {
    if filter.len() != db.len() {
        return Err(FillError::FilterSize {
            filter: filter.len(),
            games: db.len(),
        });
    }
    let total = u64::from(filter.count());
    let mut done = 0u64;
    let mut report = FillReport::default();
    for n in filter.iter_games() {
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        match db.game(n) {
            Ok(game) => {
                let mut part = FillReport::default();
                let outcome = feed_game_into(mask, &game, side, &mut part);
                // What a failed game did before failing still counts.
                report.moves += part.moves;
                report.changed |= part.changed;
                report.mask_full |= part.mask_full;
                match outcome {
                    Ok(()) => report.games += 1,
                    Err(_) => report.skipped += 1,
                }
            }
            Err(_) => report.skipped += 1,
        }
        done += 1;
        progress.progress(done, total);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::mask::MaskPosition;
    use mychess_core::SanMode;

    fn play(game: &mut Game, parent: NodeId, san: &str) -> NodeId {
        let pos = game.position_at(parent).unwrap().clone();
        let mv = pos.parse_san(san, SanMode::Strict).unwrap().mv;
        game.add_move(parent, mv).unwrap()
    }

    fn play_root(game: &mut Game, san: &str) -> NodeId {
        let root = game.root();
        play(game, root, san)
    }

    fn start_entry(mask: &TreeMask) -> &MaskPosition {
        mask.position(&Position::new()).unwrap()
    }

    #[test]
    fn standard_nag_replaces_and_others_go_to_the_comment() {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e4");
        g.add_nag(e4, Nag::new(2).unwrap()).unwrap();
        g.set_comment(e4, "[%eval 0.3] sharp [%clk 0:01:00]")
            .unwrap();
        let e5 = play(&mut g, e4, "e5");
        g.add_nag(e5, Nag::new(14).unwrap()).unwrap();
        g.set_comment(e5, "+0.35").unwrap();
        let nf3 = play(&mut g, e5, "Nf3");
        g.add_nag(nf3, Nag::new(14).unwrap()).unwrap();
        g.set_comment(nf3, "plan").unwrap();
        let mut mask = TreeMask::new();
        let r = feed_game(&mut mask, &g, FillSide::All).unwrap();
        assert!(r.changed);
        assert_eq!(r.moves, 3);
        let m = &start_entry(&mask).moves[0];
        assert_eq!(m.nag, Some(MaskNag::Poor), "? is a standard NAG (D1824 a)");
        assert_eq!(m.comment, "sharp");
        let pos = g.position_at(e4).unwrap();
        let m = &mask.position(pos).unwrap().moves[0];
        assert_eq!(m.nag, None);
        assert_eq!(
            m.comment, "+=",
            "a numeric score is dropped; the NAG goes in"
        );
        let pos = g.position_at(e5).unwrap();
        assert_eq!(mask.position(pos).unwrap().moves[0].comment, "plan (+=)");
    }

    #[test]
    fn running_twice_changes_nothing_and_comments_merge_without_repeats() {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e4");
        g.set_comment(e4, "line A\nline B").unwrap();
        let mut mask = TreeMask::new();
        assert!(feed_game(&mut mask, &g, FillSide::All).unwrap().changed);
        let snapshot = mask.clone();
        assert!(!feed_game(&mut mask, &g, FillSide::All).unwrap().changed);
        assert_eq!(mask, snapshot);
        // Another game with a different comment on the same move.
        g.set_comment(e4, "line B").unwrap();
        assert!(!feed_game(&mut mask, &g, FillSide::All).unwrap().changed);
        g.set_comment(e4, "line C").unwrap();
        assert!(feed_game(&mut mask, &g, FillSide::All).unwrap().changed);
        assert_eq!(
            start_entry(&mask).moves[0].comment,
            "line A\nline B\nline C"
        );
    }

    #[test]
    fn initial_comment_goes_to_the_start_position_and_variations_are_visited() {
        let mut g = Game::new();
        g.set_initial_comment("  intro [%csl Ge4] ");
        let e4 = play_root(&mut g, "e4");
        let d4 = play_root(&mut g, "d4");
        let _ = (e4, d4);
        let mut mask = TreeMask::new();
        feed_game(&mut mask, &g, FillSide::All).unwrap();
        let e = start_entry(&mask);
        assert_eq!(e.comment, "intro");
        assert_eq!(e.moves.len(), 2);
        assert!(
            !e.moves.iter().any(MaskMove::is_null),
            "null is never created"
        );
        // An initial comment alone creates the position, without moves.
        let mut only = Game::new();
        only.set_initial_comment("hello");
        let mut m2 = TreeMask::new();
        let r = feed_game(&mut m2, &only, FillSide::All).unwrap();
        assert!(r.changed);
        assert_eq!(start_entry(&m2).comment, "hello");
    }

    #[test]
    fn game_from_a_given_fen_uses_its_start_position() {
        let pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        let mut g = Game::from_position(pos.clone());
        g.set_initial_comment("endgame");
        play_root(&mut g, "e4");
        let mut mask = TreeMask::new();
        feed_game(&mut mask, &g, FillSide::All).unwrap();
        let e = mask.position(&pos).unwrap();
        assert_eq!((e.comment.as_str(), e.moves.len()), ("endgame", 1));
    }

    #[test]
    fn line_and_sides() {
        let mut g = Game::new();
        let e4 = play_root(&mut g, "e4");
        let e5 = play(&mut g, e4, "e5");
        let nf3 = play(&mut g, e5, "Nf3");
        let nc6 = play(&mut g, nf3, "Nc6");
        // A variation after e5, and the cursor inside it.
        let nc3 = play(&mut g, e5, "Nc3");
        let _ = nc6;
        let mut mask = TreeMask::new();
        let r = feed_line(&mut mask, &g, nc3, FillSide::All).unwrap();
        assert_eq!(r.moves, 3);
        assert_eq!(
            mask.position(g.position_at(e5).unwrap())
                .unwrap()
                .moves
                .len(),
            1
        );
        assert_eq!(
            mask.position(g.position_at(e5).unwrap()).unwrap().moves[0].san,
            "Nc3"
        );
        let mut white = TreeMask::new();
        assert_eq!(
            feed_line(&mut white, &g, nc3, FillSide::White)
                .unwrap()
                .moves,
            2
        );
        let mut black = TreeMask::new();
        assert_eq!(
            feed_line(&mut black, &g, nc3, FillSide::Black)
                .unwrap()
                .moves,
            1
        );
        let mut none = TreeMask::new();
        let r = feed_line(&mut none, &g, g.root(), FillSide::All).unwrap();
        assert!(
            !r.changed && none.is_empty(),
            "start of the game: nothing to do"
        );
    }

    #[test]
    fn chess960_is_refused() {
        let g = Game::from_position(Position::chess960_start(0).unwrap());
        let mut mask = TreeMask::new();
        assert!(matches!(
            feed_game(&mut mask, &g, FillSide::All),
            Err(FillError::NotStandard)
        ));
        assert!(mask.is_empty());
    }

    #[test]
    fn score_detection() {
        for s in ["+0.35", "(-1.2)", "[+3]", "-0"] {
            assert!(is_score(s), "{s}");
        }
        for s in ["", "0.3", "+", "+a", "+1.", "good +1", "e4"] {
            assert!(!is_score(s), "{s}");
        }
    }
}
