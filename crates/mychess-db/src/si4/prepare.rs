// Derived from Scid vs PC 4.27, src/game.cpp (Game::Encode) and
// src/tkscid.cpp (sc_savegame).
// Copyright (c) 2000-2003 Shane Hudson. All rights reserved (notice of
// game.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! [`PreparedGame`]: a game ready to be stored, the pure half of "save game"
//! (`docs/specs/m3-2-escrita-si4.md`, "Arquitetura"). Encoding does not touch
//! any file; the writer only appends the record and fills in the IDs.

use mychess_core::{Game, Rating, RatingType};

use super::derive::{decode_count, derive_mainline, encode_count};
use super::entry::{GameFlags, IndexEntry};
use super::error::{Result, Si4Error};
use super::names_write::truncate_name;
use super::raw::RawMove;
use super::record_write::{encode_record, raw_from_game, start_list_position};

/// The five names of a game, in the order Scid adds them: White, Black,
/// Event, Site, Round.
pub const NAME_FIELDS: usize = 5;

/// A game encoded for storage: the `.sg4` record, the index entry with
/// everything that does not depend on the database, and the raw bytes of the
/// names. The offset, the length and the name IDs are filled in when the game
/// is saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedGame {
    record: Vec<u8>,
    entry: IndexEntry,
    names: [Vec<u8>; NAME_FIELDS],
    truncated_names: bool,
}

fn name_bytes(text: &str, what: &'static str, truncated: &mut bool) -> Result<Vec<u8>> {
    if text.contains('\0') {
        return Err(Si4Error::InvalidText { what });
    }
    let (kept, cut) = truncate_name(text);
    *truncated |= cut;
    Ok(kept.as_bytes().to_vec())
}

fn rating_parts(rating: Option<Rating>) -> (u16, u8) {
    match rating {
        None => (0, 0),
        Some(r) => {
            let kind = RatingType::ALL
                .iter()
                .position(|&k| k == r.kind)
                .and_then(|i| u8::try_from(i).ok())
                .unwrap_or(0);
            (r.value.get(), kind)
        }
    }
}

impl PreparedGame {
    /// Encodes a game (`Game::Encode`).
    ///
    /// `flags` are the user flags of the new game; the derived ones (`START`,
    /// `PROMO`, `UPROMO`) are computed and anything outside
    /// [`GameFlags::EDITABLE`] in `flags` is ignored. Names longer than 255
    /// bytes are shortened on a character boundary, which
    /// [`PreparedGame::names_truncated`] reports.
    ///
    /// Errors are the ones of [`raw_from_game`] and [`encode_record`], plus
    /// [`Si4Error::InvalidText`] for a name with a NUL byte.
    pub fn from_game(game: &Game, flags: GameFlags) -> Result<PreparedGame> {
        let raw = raw_from_game(game)?;
        let encoded = encode_record(&raw)?;
        let start = start_list_position(raw.start_fen.as_deref())?;
        let standard_start = raw.start_fen.is_none();
        let main_line: Vec<RawMove> = raw.mainline().iter().map(|n| n.mv).collect();
        let derived = derive_mainline(&start, standard_start, &main_line)?;

        let tags = game.tags();
        let mut truncated = false;
        let names = [
            name_bytes(&tags.white, "White name", &mut truncated)?,
            name_bytes(&tags.black, "Black name", &mut truncated)?,
            name_bytes(&tags.event, "Event name", &mut truncated)?,
            name_bytes(&tags.site, "Site name", &mut truncated)?,
            name_bytes(&tags.round, "Round name", &mut truncated)?,
        ];

        let mut entry_flags = flags & GameFlags::EDITABLE;
        entry_flags.set(GameFlags::START, !standard_start);
        entry_flags.set(GameFlags::PROMOTION, derived.promotions);
        entry_flags.set(GameFlags::UNDER_PROMOTION, derived.under_promotions);
        let scale = |n: u32| decode_count(encode_count(n));
        let (white_elo, white_rating_type) = rating_parts(tags.white_rating);
        let (black_elo, black_rating_type) = rating_parts(tags.black_rating);
        let entry = IndexEntry {
            length: u32::try_from(encoded.bytes.len()).unwrap_or(u32::MAX),
            flags: entry_flags,
            variation_count: scale(encoded.variation_count),
            comment_count: scale(encoded.comment_count),
            nag_count: scale(encoded.nag_count),
            result: tags.result,
            eco_code: tags.eco.map_or(0, mychess_core::Eco::to_scid_code),
            date: tags.date,
            event_date: tags.event_date,
            white_elo,
            white_rating_type,
            black_elo,
            black_rating_type,
            final_mat_sig: derived.final_mat_sig,
            stored_line_code: derived.stored_line_code,
            num_half_moves: u16::try_from(derived.num_half_moves % 1024).unwrap_or(0),
            home_pawn_data: derived.home_pawn_data,
            ..IndexEntry::default()
        };
        Ok(PreparedGame {
            record: encoded.bytes,
            entry,
            names,
            truncated_names: truncated,
        })
    }

    /// The bytes of the `.sg4` record.
    pub fn record(&self) -> &[u8] {
        &self.record
    }

    /// The index entry; the offset and the name IDs are 0 until saved.
    pub fn entry(&self) -> &IndexEntry {
        &self.entry
    }

    /// The raw bytes of the names: White, Black, Event, Site, Round.
    pub fn names(&self) -> &[Vec<u8>; NAME_FIELDS] {
        &self.names
    }

    /// Whether a name had to be shortened to 255 bytes.
    pub fn names_truncated(&self) -> bool {
        self.truncated_names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::{Elo, GameResult, PartialDate, Position, Role};

    fn play(game: &mut Game, parent: mychess_core::NodeId, uci: &str) -> mychess_core::NodeId {
        let mv = game.position_at(parent).unwrap().parse_uci(uci).unwrap();
        game.add_move(parent, mv).unwrap()
    }

    #[test]
    fn a_short_game_fills_the_entry() {
        let mut game = Game::new();
        let root = game.root();
        let e4 = play(&mut game, root, "e2e4");
        play(&mut game, e4, "e7e5");
        {
            let tags = game.tags_mut();
            tags.white = "Anand".into();
            tags.black = "Carlsen".into();
            tags.result = GameResult::Draw;
            tags.date = PartialDate::new(Some(2013), Some(11), None).unwrap();
            tags.white_rating = Some(Rating::new(Elo::new(2775).unwrap(), RatingType::Elo));
            tags.black_rating = Some(Rating::new(Elo::new(2870).unwrap(), RatingType::Dwz));
        }
        let prepared =
            PreparedGame::from_game(&game, GameFlags::DELETE | GameFlags::START).unwrap();
        let e = prepared.entry();
        assert_eq!(prepared.record(), &[0, 0, 0xCF, 0xCF, 0x0F]);
        assert_eq!(e.length, 5);
        assert_eq!(e.flags, GameFlags::DELETE);
        assert_eq!((e.white_elo, e.white_rating_type), (2775, 0));
        assert_eq!((e.black_elo, e.black_rating_type), (2870, 5));
        assert_eq!(e.num_half_moves, 2);
        assert_eq!(e.home_pawn_data[0], 2);
        assert_ne!(e.stored_line_code, 0, "1.e4 e5 is a stored line prefix");
        assert_eq!(e.result, GameResult::Draw);
        assert_eq!(prepared.names()[0], b"Anand");
        assert_eq!(prepared.names()[4], b"?");
        assert!(!prepared.names_truncated());
        // The entry can be written and read back.
        let back = IndexEntry::parse(&e.to_bytes()).unwrap();
        assert_eq!(back.date, e.date);
        assert_eq!(back.final_mat_sig, e.final_mat_sig);
    }

    #[test]
    fn promotions_and_start_positions_set_the_derived_flags() {
        let pos = Position::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mut game = Game::from_position(pos);
        let promo = game
            .position_at(game.root())
            .unwrap()
            .find_move(
                mychess_core::Square::A7,
                mychess_core::Square::A8,
                Some(Role::Rook),
            )
            .unwrap();
        game.add_move(game.root(), promo).unwrap();
        let prepared = PreparedGame::from_game(&game, GameFlags::empty()).unwrap();
        let flags = prepared.entry().flags;
        assert!(
            flags.contains(GameFlags::START | GameFlags::PROMOTION | GameFlags::UNDER_PROMOTION)
        );
        assert_eq!(prepared.entry().home_pawn_data, [0; 9]);
        assert_eq!(prepared.entry().stored_line_code, 0);
        assert_eq!(prepared.record()[prepared.record().len() - 1], 0x0F);
        // Record flags: own start, promotions, under-promotions.
        assert_eq!(prepared.record()[1], 7);
    }

    #[test]
    fn names_with_nul_are_refused_and_long_ones_are_cut() {
        let mut game = Game::new();
        game.tags_mut().white = "a\0b".into();
        assert!(matches!(
            PreparedGame::from_game(&game, GameFlags::empty()),
            Err(Si4Error::InvalidText { .. })
        ));
        game.tags_mut().white = "w".repeat(300);
        let prepared = PreparedGame::from_game(&game, GameFlags::empty()).unwrap();
        assert_eq!(prepared.names()[0].len(), 255);
        assert!(prepared.names_truncated());
    }

    #[test]
    fn counts_are_stored_on_the_scale() {
        let mut game = Game::new();
        let root = game.root();
        let mut node = play(&mut game, root, "e2e4");
        game.set_comment(node, "c").unwrap();
        for _ in 0..13 {
            game.add_nag(node, mychess_core::Nag::new(1).unwrap()).ok();
        }
        node = play(&mut game, node, "e7e5");
        let _ = node;
        let prepared = PreparedGame::from_game(&game, GameFlags::empty()).unwrap();
        // A repeated NAG is stored once; one comment; no variation.
        assert_eq!(prepared.entry().nag_count, 1);
        assert_eq!(prepared.entry().comment_count, 1);
        assert_eq!(prepared.entry().variation_count, 0);
    }
}
