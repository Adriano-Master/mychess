//! Piece sets: the SVG handles of the 12 pieces.

use iced::widget::svg;
use mychess_core::{Color, Piece, Role};

use crate::assets;

/// Which set of pieces to draw. The preferences store [`kebab_name`]
/// (the directory name under `assets/pieces/`).
///
/// [`kebab_name`]: PieceSetId::kebab_name
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PieceSetId {
    /// Colin M.L. Burnett's set (the default).
    #[default]
    Cburnett,
    /// Armando Hernandez Marroquin's Merida.
    Merida,
    /// Alexis Luengas' Chessnut.
    Chessnut,
    /// `RhosGFX`.
    Rhosgfx,
    /// Maurizio Monge's Fantasy.
    Fantasy,
    /// Maurizio Monge's Spatial.
    Spatial,
    /// Maurizio Monge's Celtic.
    Celtic,
    /// Maxime Chupin's `MPChess`.
    Mpchess,
}

impl PieceSetId {
    /// Every set, in menu order.
    pub const ALL: [PieceSetId; 8] = [
        PieceSetId::Cburnett,
        PieceSetId::Merida,
        PieceSetId::Chessnut,
        PieceSetId::Rhosgfx,
        PieceSetId::Fantasy,
        PieceSetId::Spatial,
        PieceSetId::Celtic,
        PieceSetId::Mpchess,
    ];

    /// The id used in the preferences file.
    pub fn kebab_name(self) -> &'static str {
        match self {
            PieceSetId::Cburnett => "cburnett",
            PieceSetId::Merida => "merida",
            PieceSetId::Chessnut => "chessnut",
            PieceSetId::Rhosgfx => "rhosgfx",
            PieceSetId::Fantasy => "fantasy",
            PieceSetId::Spatial => "spatial",
            PieceSetId::Celtic => "celtic",
            PieceSetId::Mpchess => "mpchess",
        }
    }

    /// Parses [`kebab_name`](Self::kebab_name); `None` for an unknown id.
    pub fn from_kebab_name(name: &str) -> Option<PieceSetId> {
        PieceSetId::ALL
            .into_iter()
            .find(|id| id.kebab_name() == name)
    }

    /// The Fluent key of the display name.
    pub fn name_key(self) -> &'static str {
        match self {
            PieceSetId::Cburnett => "piece-set-cburnett",
            PieceSetId::Merida => "piece-set-merida",
            PieceSetId::Chessnut => "piece-set-chessnut",
            PieceSetId::Rhosgfx => "piece-set-rhosgfx",
            PieceSetId::Fantasy => "piece-set-fantasy",
            PieceSetId::Spatial => "piece-set-spatial",
            PieceSetId::Celtic => "piece-set-celtic",
            PieceSetId::Mpchess => "piece-set-mpchess",
        }
    }

    /// The embedded SVG files of the set.
    pub(crate) fn files(self) -> &'static assets::PieceFiles {
        match self {
            PieceSetId::Cburnett => &assets::CBURNETT,
            PieceSetId::Merida => &assets::MERIDA,
            PieceSetId::Chessnut => &assets::CHESSNUT,
            PieceSetId::Rhosgfx => &assets::RHOSGFX,
            PieceSetId::Fantasy => &assets::FANTASY,
            PieceSetId::Spatial => &assets::SPATIAL,
            PieceSetId::Celtic => &assets::CELTIC,
            PieceSetId::Mpchess => &assets::MPCHESS,
        }
    }
}

/// The twelve SVG handles of a set, created once.
#[derive(Debug, Clone)]
pub struct PieceSet {
    id: PieceSetId,
    handles: Vec<svg::Handle>,
}

fn index(piece: Piece) -> usize {
    let color = match piece.color {
        Color::White => 0,
        Color::Black => 6,
    };
    let role = match piece.role {
        Role::King => 0,
        Role::Queen => 1,
        Role::Rook => 2,
        Role::Bishop => 3,
        Role::Knight => 4,
        Role::Pawn => 5,
    };
    color + role
}

impl PieceSet {
    /// Loads a set from the embedded SVG files.
    pub fn load(id: PieceSetId) -> PieceSet {
        let handles = id
            .files()
            .iter()
            .map(|(_, bytes)| svg::Handle::from_memory(*bytes))
            .collect();
        PieceSet { id, handles }
    }

    /// Which set this is.
    pub fn id(&self) -> PieceSetId {
        self.id
    }

    /// The handle of a piece.
    pub fn handle(&self, piece: Piece) -> Option<&svg::Handle> {
        self.handles.get(index(piece))
    }
}

impl Default for PieceSet {
    fn default() -> PieceSet {
        PieceSet::load(PieceSetId::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_set_has_twelve_handles_and_the_files_are_svg() {
        for id in PieceSetId::ALL {
            let set = PieceSet::load(id);
            assert_eq!(set.id(), id);
            for color in [Color::White, Color::Black] {
                for role in Role::ALL {
                    assert!(set.handle(Piece { color, role }).is_some(), "{id:?}");
                }
            }
            for (name, bytes) in id.files() {
                assert!(
                    std::str::from_utf8(bytes).is_ok_and(|text| text.contains("<svg")),
                    "{id:?} {name}"
                );
            }
        }
    }

    #[test]
    fn ids_round_trip_and_unknown_ones_are_rejected() {
        for id in PieceSetId::ALL {
            assert_eq!(PieceSetId::from_kebab_name(id.kebab_name()), Some(id));
            assert_eq!(id.name_key(), format!("piece-set-{}", id.kebab_name()));
        }
        assert_eq!(PieceSetId::from_kebab_name("staunty"), None);
        assert_eq!(PieceSetId::from_kebab_name("Cburnett"), None);
        assert_eq!(PieceSetId::from_kebab_name(""), None);
        assert_eq!(PieceSetId::default(), PieceSetId::Cburnett);
    }

    #[test]
    fn asset_names_match_the_index_order() {
        let expected = [
            "wK", "wQ", "wR", "wB", "wN", "wP", "bK", "bQ", "bR", "bB", "bN", "bP",
        ];
        for id in PieceSetId::ALL {
            for (i, (name, _)) in id.files().iter().enumerate() {
                assert_eq!(*name, expected[i], "{id:?}");
            }
        }
        let wn = Piece {
            color: Color::White,
            role: Role::Knight,
        };
        let bk = Piece {
            color: Color::Black,
            role: Role::King,
        };
        assert_eq!(assets::CBURNETT[index(wn)].0, "wN");
        assert_eq!(assets::CBURNETT[index(bk)].0, "bK");
    }
}
