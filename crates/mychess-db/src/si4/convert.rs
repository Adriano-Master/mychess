// Derived from Scid vs PC 4.27, src/game.cpp (LoadStandardTags, record decoding).
// Copyright (c) 2000-2003 Shane Hudson. All rights reserved (notice of
// game.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Conversion of a decoded record into a [`mychess_core::Game`]
//! (decision D264).
//!
//! The raw moves are matched with the legal moves of the core position, so a
//! move that is not legal is an error. Walking the variations uses an explicit
//! work stack: no recursion.

use mychess_core::{Game, GameError, Move, NodeId, PartialDate, Position, Tags};

use super::entry::IndexEntry;
use super::error::{Result, Si4Error};
use super::names::{NameBase, NameKind};
use super::raw::{RawGame, RawMove};
use super::text::bytes_to_string;

/// What the conversion had to do to variations that repeat a move.
///
/// The core merges equal sibling moves, so a variation whose first move is
/// already a child of the same parent is merged into that node (its other
/// moves become children of it). When the repeated node has no continuation
/// the rest of the variation would turn into the main line, so that
/// variation is dropped instead.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConversionReport {
    /// Variations whose first move was merged with an existing move.
    pub merged_variations: u32,
    /// Variations (or their tails) dropped because merging would have
    /// extended the main line. `raw_game` still has them.
    pub discarded_variations: u32,
}

/// Maps a core error at `ply` to the right error of this module.
fn game_error(game: u32, ply: u32, error: GameError) -> Si4Error {
    match error {
        GameError::IllegalMove(_) => Si4Error::IllegalMove { game, ply },
        source => Si4Error::Game { game, source },
    }
}

/// Builds the game of record number `game` from its index entry, the name
/// base and the decoded record.
pub(crate) fn build_game(
    game: u32,
    entry: &IndexEntry,
    names: &NameBase,
    raw: &RawGame,
) -> Result<(Game, ConversionReport)> {
    let start = match &raw.start_fen {
        Some(fen) => Position::from_fen(&bytes_to_string(fen))
            .map_err(|source| Si4Error::IllegalStart { game, source })?,
        None => Position::new(),
    };
    let mut out = Game::from_position(start);
    *out.tags_mut() = build_tags(game, entry, names, raw)?;
    let report = add_lines(game, &mut out, raw)?;
    Ok((out, report))
}

/// The tags of a game: the seven-tag roster from the index and the name base
/// (Scid's `LoadStandardTags`) plus the extra tags of the record.
///
/// Extra tags the core refuses (a name that is not a PGN symbol, a name that
/// has a dedicated field) are dropped; line breaks and NULs in values become
/// spaces. `EventDate` tags fill the event date when the index has none.
fn build_tags(game: u32, entry: &IndexEntry, names: &NameBase, raw: &RawGame) -> Result<Tags> {
    let name = |kind: NameKind, id: u32| {
        names
            .get(kind, id)
            .map(|bytes| bytes_to_string(bytes).into_owned())
            .ok_or(Si4Error::CorruptIndex {
                game,
                reason: "name id out of range",
            })
    };
    let mut tags = Tags::new();
    tags.event = name(NameKind::Event, entry.event_id)?;
    tags.site = name(NameKind::Site, entry.site_id)?;
    tags.white = name(NameKind::Player, entry.white_id)?;
    tags.black = name(NameKind::Player, entry.black_id)?;
    tags.round = name(NameKind::Round, entry.round_id)?;
    tags.date = entry.date;
    tags.event_date = raw.event_date.unwrap_or(entry.event_date);
    tags.result = entry.result;
    tags.white_rating = entry.white_rating();
    tags.black_rating = entry.black_rating();
    tags.eco = entry.eco();
    for (tag_name, value) in &raw.extra_tags {
        let tag_name = bytes_to_string(tag_name);
        let value = bytes_to_string(value).replace(['\r', '\n', '\0'], " ");
        if tag_name == "EventDate" {
            if tags.event_date.is_unknown() {
                tags.event_date = PartialDate::parse_lenient(&value);
            }
            continue;
        }
        // Lossy by design: see the function documentation.
        tags.set_extra(&tag_name, &value).ok();
    }
    Ok(tags)
}

/// Work item: build `line` starting from the node `parent`.
struct Task {
    line: usize,
    parent: NodeId,
}

fn add_lines(game: u32, out: &mut Game, raw: &RawGame) -> Result<ConversionReport> {
    let mut report = ConversionReport::default();
    let Some(main) = raw.lines.first() else {
        return Ok(report);
    };
    if let Some(comment) = &main.start_comment {
        out.set_initial_comment(&bytes_to_string(comment));
    }
    let mut tasks = vec![Task {
        line: 0,
        parent: out.root(),
    }];
    while let Some(task) = tasks.pop() {
        let Some(line) = raw.lines.get(task.line) else {
            continue;
        };
        let mut parent = task.parent;
        let base_ply = out.ply(parent).map_err(|e| game_error(game, 0, e))?;
        for (i, node) in line.moves.iter().enumerate() {
            let ply = base_ply.saturating_add(u32::try_from(i + 1).unwrap_or(u32::MAX));
            let err = |e| game_error(game, ply, e);
            let mv = core_move(out, parent, node.mv).ok_or(Si4Error::IllegalMove { game, ply })?;
            let (id, created) = out.add_move_new(parent, mv).map_err(err)?;
            // The start comment of the main line is the game's initial one.
            let start_comment = line
                .start_comment
                .as_ref()
                .filter(|_| i == 0 && task.line != 0);
            let existing = if created {
                None
            } else {
                Some((
                    out.comment(id).map_err(err)?.is_some(),
                    out.comment_before(id).map_err(err)?.is_some(),
                    !out.nags(id).map_err(err)?.is_empty(),
                ))
            };
            // A merged node keeps what it already has.
            let (has_comment, has_before, has_nags) = existing.unwrap_or_default();
            if let Some(comment) = start_comment.filter(|_| !has_before) {
                out.set_comment_before(id, &bytes_to_string(comment))
                    .map_err(err)?;
            }
            if let Some(comment) = node.comment.as_ref().filter(|_| !has_comment) {
                out.set_comment(id, &bytes_to_string(comment))
                    .map_err(err)?;
            }
            if !has_nags {
                for nag in &node.nags {
                    out.add_nag(id, *nag).map_err(err)?;
                }
            }
            // Variations are alternatives to this move: children of `parent`.
            // Pushed in reverse so they are built in order.
            for &variation in node.variations.iter().rev() {
                tasks.push(Task {
                    line: variation,
                    parent,
                });
            }
            parent = id;
            if !created {
                let dead_end = out.children(id).map_err(err)?.is_empty();
                if dead_end && i + 1 < line.moves.len() {
                    // Merging would extend the line the node belongs to.
                    report.discarded_variations += 1;
                    break;
                }
                if i == 0 {
                    report.merged_variations += 1;
                }
            }
        }
    }
    Ok(report)
}

/// Matches a raw move with a legal move of the position at `parent`.
fn core_move(out: &Game, parent: NodeId, mv: RawMove) -> Option<Move> {
    if mv.is_null() {
        return Some(Move::Null);
    }
    out.position_at(parent)
        .ok()?
        .find_move(mv.from, mv.to, mv.promotion)
}
