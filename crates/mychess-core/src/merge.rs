//! Merging one game into another as variations ([`Game::merge_game`]).
//!
//! Scid: `src/tkscid.cpp`, `sc_game_merge` (lines 8082-8222). The Scid finds
//! the **deepest** position of the current game that also occurs somewhere in
//! the main line of the other game, opens a variation there and copies the
//! other game's moves into it. This module follows that algorithm, with the
//! deviations of `docs/specs/m7-6-mesclar-partida.md`:
//!
//! * the start position may match (the first move gets an alternative; the
//!   Scid never compares ply 0) (D3855);
//! * at the end of the main line the moves **continue the main line** instead
//!   of repeating the last move in a variation, because the tree never holds
//!   two identical sibling moves (D3856);
//! * merging is idempotent: equal moves fuse into the existing nodes and the
//!   comment is not duplicated (D3857);
//! * nothing to add is an error and changes nothing (D3858);
//! * a non-standard start position is accepted on both sides (D3859).
//!
//! The operation is pure (no I/O) and atomic: on error the game is unchanged.

use std::collections::HashMap;

use thiserror::Error;

use crate::{Game, GameError, NodeId, Position};

/// Options of [`Game::merge_game`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeOptions {
    /// Last ply (absolute, counted in the **other** game) to copy; `None`
    /// copies up to the end. At least one move is always copied.
    pub end_ply: Option<u32>,
    /// Text put in the comment before the first merged move (already
    /// translated by the caller); `None` or blank adds no comment.
    pub comment: Option<String>,
}

/// What [`Game::merge_game`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeReport {
    /// Matching ply in the current game (0 = the start position).
    pub match_ply: u32,
    /// Matching ply in the other game.
    pub other_ply: u32,
    /// Moves that were created.
    pub added: u32,
    /// Moves that already existed and were fused.
    pub reused: u32,
    /// The first merged node (new or reused).
    pub first: NodeId,
    /// The matching position was the end of the main line, so the moves
    /// continue the main line.
    pub extended_mainline: bool,
    /// `end_ply` cut the other game short.
    pub truncated: bool,
}

/// Why [`Game::merge_game`] refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum MergeError {
    /// The current game has no moves.
    #[error("the current game has no moves")]
    NoMoves,
    /// No position of the current game occurs in the other game.
    #[error("no matching position found")]
    NoMatch,
    /// The other game has no moves after the matching position (or the
    /// cut-off leaves none).
    #[error("the other game has no moves after the matching position")]
    NothingToAdd,
    /// A tree operation failed (for example a castling move that the
    /// castling mode of the current game does not accept). Nothing changed.
    #[error(transparent)]
    Game(#[from] GameError),
}

/// Pieces of the description comment of a merged game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeCommentParts {
    /// `White` tag.
    pub white: String,
    /// `Black` tag.
    pub black: String,
    /// Long result: `1-0`, `0-1`, `1/2-1/2` or `*`.
    pub result: &'static str,
    /// Halfmoves in the main line of the whole game.
    pub half_moves: u32,
    /// `Event` tag.
    pub event: String,
    /// Year of the `Date` tag; 0 when unknown.
    pub year: u16,
}

impl MergeCommentParts {
    /// Full moves of the whole game, `(half_moves + 1) / 2`.
    pub fn full_moves(&self) -> u32 {
        self.half_moves.div_ceil(2)
    }

    /// The comment in neutral English:
    /// `White -- Black Result[ (N moves)]\nEvent, year`. The `(N moves)` part
    /// appears when `truncated`. The application builds the translated text
    /// from the fields instead.
    pub fn english(&self, truncated: bool) -> String {
        let cut = if truncated {
            format!(" ({} moves)", self.full_moves())
        } else {
            String::new()
        };
        format!(
            "{} -- {} {}{}\n{}, {}",
            self.white, self.black, self.result, cut, self.event, self.year
        )
    }
}

/// The description of `other` for the merge comment.
///
/// Scid: `sc_game_merge` (8193-8205).
pub fn merge_comment_parts(other: &Game) -> MergeCommentParts {
    let tags = other.tags();
    MergeCommentParts {
        white: tags.white.clone(),
        black: tags.black.clone(),
        result: tags.result.as_str(),
        half_moves: u32::try_from(other.mainline().count()).unwrap_or(u32::MAX),
        event: tags.event.clone(),
        year: tags.date.year().unwrap_or(0),
    }
}

impl Game {
    /// Merges the main line of `other` into this game as a variation.
    ///
    /// Finds the deepest position of this game's main line (ply 0, the start,
    /// included) that occurs anywhere in `other`'s main line (the latest
    /// occurrence wins), then adds `other`'s moves from there as an
    /// alternative to the move that follows in this game. Moves that already
    /// exist are fused, not duplicated. If the match is the end of this
    /// game's main line, the moves continue the main line
    /// ([`MergeReport::extended_mainline`]). Only moves are copied (no
    /// comments, NAGs or variations of `other`). `other` may have any start
    /// position; positions are compared like [`Position`] equality (board,
    /// side to move, castling rights, legal en passant).
    ///
    /// Atomic: on error this game is unchanged.
    ///
    /// ```
    /// use mychess_core::{Game, MergeOptions, Position, SanMode};
    ///
    /// fn game(sans: &[&str]) -> Result<Game, Box<dyn std::error::Error>> {
    ///     let mut g = Game::new();
    ///     let mut at = g.root();
    ///     for san in sans {
    ///         let mv = g.position_at(at)?.parse_san(san, SanMode::Strict)?.mv;
    ///         at = g.add_move(at, mv)?;
    ///     }
    ///     Ok(g)
    /// }
    /// let mut a = game(&["e4", "e5", "Nf3", "Nc6", "Bb5"])?;
    /// let b = game(&["e4", "e5", "Nf3", "Nc6", "Bc4"])?;
    /// let report = a.merge_game(&b, &MergeOptions::default())?;
    /// assert_eq!((report.match_ply, report.added), (4, 1));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Scid: `src/tkscid.cpp`, `sc_game_merge`.
    pub fn merge_game(
        &mut self,
        other: &Game,
        opts: &MergeOptions,
    ) -> Result<MergeReport, MergeError> {
        let own: Vec<NodeId> = std::iter::once(self.root())
            .chain(self.mainline())
            .collect();
        if own.len() < 2 {
            return Err(MergeError::NoMoves);
        }
        let theirs: Vec<NodeId> = std::iter::once(other.root())
            .chain(other.mainline())
            .collect();

        // Position -> latest ply in `other` (ties go to the later ply).
        let mut index: HashMap<&Position, usize> = HashMap::with_capacity(theirs.len());
        for (ply, &id) in theirs.iter().enumerate() {
            index.insert(other.position_at(id)?, ply);
        }
        // Deepest ply of this game that occurs in `other`.
        let mut found: Option<(usize, usize)> = None;
        for (ply, &id) in own.iter().enumerate() {
            if let Some(&n) = index.get(self.position_at(id)?) {
                found = Some((ply, n));
            }
        }
        let (match_ply, other_ply) = found.ok_or(MergeError::NoMatch)?;

        let other_len = theirs.len() - 1;
        let wanted = opts
            .end_ply
            .map_or(usize::MAX, |e| usize::try_from(e).unwrap_or(usize::MAX));
        // Scid: at least one move is shown (`if (ply >= endPly) endPly = ply + 1`).
        let end = wanted.max(other_ply.saturating_add(1)).min(other_len);
        if end <= other_ply {
            return Err(MergeError::NothingToAdd);
        }

        let extended_mainline = match_ply + 1 == own.len();
        let mut parent = *own.get(match_ply).ok_or(MergeError::NoMatch)?;
        let (mut added, mut reused) = (0u32, 0u32);
        let mut first: Option<NodeId> = None;
        let mut first_created: Option<NodeId> = None;
        for &src in theirs.get(other_ply + 1..=end).unwrap_or(&[]) {
            let step = other
                .mv(src)
                .map_err(MergeError::from)
                .and_then(|mv| self.add_move_new(parent, mv).map_err(MergeError::from));
            let (node, created) = match step {
                Ok(v) => v,
                Err(err) => {
                    if let Some(c) = first_created {
                        // Roll back: everything created hangs below `c`.
                        drop(self.truncate_from(c));
                    }
                    return Err(err);
                }
            };
            if created {
                added = added.saturating_add(1);
                first_created.get_or_insert(node);
            } else {
                reused = reused.saturating_add(1);
            }
            first.get_or_insert(node);
            parent = node;
        }
        let first = first.ok_or(MergeError::NothingToAdd)?;

        if let Some(text) = opts.comment.as_deref().filter(|t| !t.trim().is_empty()) {
            self.merge_comment(first, text)?;
        }

        Ok(MergeReport {
            match_ply: u32::try_from(match_ply).unwrap_or(u32::MAX),
            other_ply: u32::try_from(other_ply).unwrap_or(u32::MAX),
            added,
            reused,
            first,
            extended_mainline,
            truncated: end < other_len,
        })
    }

    /// Puts `text` before the move `id`: replaces nothing; appends on its own
    /// line unless already contained (idempotence, D3857).
    fn merge_comment(&mut self, id: NodeId, text: &str) -> Result<(), GameError> {
        let merged = match self.comment_before(id)? {
            None => text.to_owned(),
            Some(old) if old.contains(text) => return Ok(()),
            Some(old) => format!("{old}\n{text}"),
        };
        self.set_comment_before(id, &merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GameResult, SanMode};

    fn game(sans: &str) -> Game {
        let mut g = Game::new();
        let mut at = g.root();
        for san in sans.split_whitespace() {
            let mv = g
                .position_at(at)
                .expect("node")
                .parse_san(san, SanMode::Strict)
                .expect("san")
                .mv;
            at = g.add_move(at, mv).expect("add");
        }
        g
    }

    fn main_len(g: &Game) -> usize {
        g.mainline().count()
    }

    fn san_of(g: &Game, id: NodeId) -> String {
        let parent = g.parent(id).expect("node").expect("not root");
        g.position_at(parent)
            .expect("node")
            .san(g.mv(id).expect("move"))
            .expect("san")
    }

    fn a() -> Game {
        game("e4 e5 Nf3 Nc6 Bb5 a6")
    }
    fn b() -> Game {
        let mut g = game("e4 e5 Nf3 Nc6 Bc4 Bc5 c3 Nf6");
        let t = g.tags_mut();
        t.white = "Alfa, A".into();
        t.black = "Beta, B".into();
        t.result = GameResult::White;
        t.event = "Aberto".into();
        t.date = crate::PartialDate::new(Some(2020), None, None).expect("date");
        g
    }

    fn opts(end: Option<u32>) -> MergeOptions {
        MergeOptions {
            end_ply: end,
            comment: Some("note".into()),
        }
    }

    #[test]
    fn merges_variation_at_deepest_match() {
        let mut g = a();
        let r = g.merge_game(&b(), &opts(None)).expect("merge");
        assert_eq!((r.match_ply, r.other_ply, r.added, r.reused), (4, 4, 4, 0));
        assert!(!r.extended_mainline && !r.truncated);
        assert_eq!(main_len(&g), 6);
        assert_eq!(san_of(&g, r.first), "Bc4");
        assert!(g.is_variation(r.first).expect("node"));
        assert_eq!(g.comment_before(r.first).expect("node"), Some("note"));
        assert_eq!(g.node_count(), 1 + 6 + 4);
    }

    #[test]
    fn end_ply_cuts_with_minimum_of_one_move() {
        let mut g = a();
        let r = g.merge_game(&b(), &opts(Some(6))).expect("merge");
        assert_eq!((r.added, r.truncated), (2, true));
        let mut g = a();
        let r = g.merge_game(&b(), &opts(Some(2))).expect("merge");
        assert_eq!((r.added, r.truncated), (1, true));
    }

    #[test]
    fn matches_by_transposition() {
        let mut g = a();
        let c = game("Nf3 e5 e4 Nc6 Bc4 Bc5");
        let r = g.merge_game(&c, &opts(None)).expect("merge");
        assert_eq!((r.match_ply, r.other_ply, r.added), (4, 4, 2));
    }

    #[test]
    fn start_position_matches_and_no_match_errors() {
        let mut g = a();
        let d = game("d4 d5 c4 e6");
        let r = g.merge_game(&d, &opts(None)).expect("merge");
        assert_eq!((r.match_ply, r.added), (0, 4));
        assert_eq!(san_of(&g, r.first), "d4");

        let mut g = a();
        let other =
            Game::from_position(Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1").expect("fen"));
        // Needs a move to be mergeable at all, but no position recurs.
        let mut other = other;
        let mv = other
            .start_position()
            .parse_san("Ra2", SanMode::Strict)
            .expect("san")
            .mv;
        other.add_move(other.root(), mv).expect("add");
        assert_eq!(g.merge_game(&other, &opts(None)), Err(MergeError::NoMatch));
    }

    #[test]
    fn end_of_mainline_extends_it() {
        let mut e = game("e4 e5");
        let r = e.merge_game(&b(), &opts(None)).expect("merge");
        assert!(r.extended_mainline);
        assert_eq!((r.added, r.match_ply), (6, 2));
        assert_eq!(main_len(&e), 8);
        assert_eq!(e.node_count(), 9, "no variations");
        assert_eq!(e.comment_before(r.first).expect("node"), Some("note"));

        let f = game("e4 e5 f4");
        let r = e.merge_game(&f, &opts(None)).expect("merge");
        assert!(!r.extended_mainline);
        assert_eq!(san_of(&e, r.first), "f4");
        assert!(e.is_variation(r.first).expect("node"));

        assert_eq!(
            e.merge_game(&b(), &opts(None)),
            Err(MergeError::NothingToAdd)
        );
    }

    #[test]
    fn identical_copy_is_nothing_to_add_and_leaves_game_unchanged() {
        let mut g = a();
        let before = g.clone();
        assert_eq!(
            g.merge_game(&a(), &opts(None)),
            Err(MergeError::NothingToAdd)
        );
        assert!(g.same_content(&before));
    }

    #[test]
    fn reverse_merge_adds_other_line() {
        let mut g = b();
        let r = g.merge_game(&a(), &opts(None)).expect("merge");
        assert_eq!((r.match_ply, r.added), (4, 2));
        assert_eq!(san_of(&g, r.first), "Bb5");
    }

    #[test]
    fn rejects_game_without_moves() {
        let mut g = Game::new();
        assert_eq!(g.merge_game(&b(), &opts(None)), Err(MergeError::NoMoves));
    }

    #[test]
    fn non_standard_start_positions_are_accepted() {
        let fen = "4k3/8/8/8/8/8/R7/4K3 w - - 0 1";
        let mk = |sans: &[&str]| {
            let mut g = Game::from_position(Position::from_fen(fen).expect("fen"));
            let mut at = g.root();
            for san in sans {
                let mv = g
                    .position_at(at)
                    .expect("node")
                    .parse_san(san, SanMode::Strict)
                    .expect("san")
                    .mv;
                at = g.add_move(at, mv).expect("add");
            }
            g
        };
        let mut x = mk(&["Ra3", "Kd7"]);
        let y = mk(&["Ra3", "Ke7", "Rb3"]);
        let r = x.merge_game(&y, &opts(None)).expect("merge");
        assert_eq!((r.match_ply, r.added), (1, 2));
        for id in x.descendants(x.root()).expect("root") {
            assert!(x.position_at(id).is_ok());
        }
        // Standard source into a FEN game without common positions.
        let mut z = mk(&["Ra3"]);
        assert_eq!(z.merge_game(&a(), &opts(None)), Err(MergeError::NoMatch));
    }

    #[test]
    fn merging_twice_is_idempotent_and_comment_not_duplicated() {
        let mut g = a();
        g.merge_game(&b(), &opts(None)).expect("first");
        let snapshot = g.clone();
        let r = g.merge_game(&b(), &opts(None)).expect("second");
        assert_eq!((r.added, r.reused), (0, 4));
        assert!(g.same_content(&snapshot));
        assert_eq!(g.comment_before(r.first).expect("node"), Some("note"));
    }

    #[test]
    fn existing_comment_gets_new_text_on_own_line() {
        let mut g = a();
        let first = g
            .merge_game(&b(), &MergeOptions::default())
            .expect("m")
            .first;
        g.set_comment_before(first, "mine").expect("set");
        g.merge_game(&b(), &opts(None)).expect("again");
        assert_eq!(g.comment_before(first).expect("node"), Some("mine\nnote"));
    }

    #[test]
    fn only_moves_are_copied() {
        let mut other = b();
        let n = other.mainline().nth(4).expect("node");
        other.set_comment(n, "hello").expect("c");
        other
            .add_nag(n, crate::Nag::new(1).expect("nag"))
            .expect("n");
        let mut g = a();
        let r = g
            .merge_game(&other, &MergeOptions::default())
            .expect("merge");
        assert_eq!(g.comment(r.first).expect("node"), None);
        assert_eq!(g.nags(r.first).expect("node").len(), 0);
        assert_eq!(g.comment_before(r.first).expect("node"), None);
    }

    #[test]
    fn comment_parts_describe_the_game() {
        let p = merge_comment_parts(&b());
        assert_eq!(
            (p.white.as_str(), p.black.as_str(), p.result, p.half_moves),
            ("Alfa, A", "Beta, B", "1-0", 8)
        );
        assert_eq!(
            (p.event.as_str(), p.year, p.full_moves()),
            ("Aberto", 2020, 4)
        );
        assert_eq!(
            p.english(true),
            "Alfa, A -- Beta, B 1-0 (4 moves)\nAberto, 2020"
        );
        assert_eq!(p.english(false), "Alfa, A -- Beta, B 1-0\nAberto, 2020");
        assert_eq!(merge_comment_parts(&a()).year, 0);
    }

    #[test]
    fn odd_half_moves_round_up_in_comment() {
        let p = merge_comment_parts(&game("e4 e5 Nf3"));
        assert_eq!(p.full_moves(), 2);
    }

    #[test]
    fn unbounded_end_ply_adds_exactly_the_reported_nodes() {
        // Standard castling right lost in the target (FEN game) but present in
        // the source: Position equality includes rights, so craft via Chess960
        // is not needed; just check that a successful merge leaves no stray
        // nodes when end_ply is huge.
        let mut g = a();
        let r = g.merge_game(&b(), &opts(Some(u32::MAX))).expect("merge");
        assert_eq!(g.node_count(), 7 + usize::try_from(r.added).expect("fits"));
    }

    #[test]
    fn cut_off_merge_at_the_end_continues_on_the_next_call() {
        // Documented consequence of D3856: the extended main line matches
        // deeper in the source the second time.
        let mut e = game("e4 e5");
        let r = e.merge_game(&b(), &opts(Some(3))).expect("first");
        assert_eq!((r.extended_mainline, r.truncated, r.added), (true, true, 1));
        let r = e.merge_game(&b(), &opts(Some(4))).expect("second");
        assert_eq!((r.match_ply, r.added), (3, 1));
        assert_eq!(main_len(&e), 4);
    }
}
