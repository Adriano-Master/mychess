//! The columns of the game list and how they sort (spec M3-4, slice 4c;
//! D506, D556 onwards; Scid: `glistFields` in `tcl/windows/gamelist.tcl`).

use serde::Serialize;

/// One of the 22 columns of the Scid game list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ListColumn {
    /// `g`: the number of the game in the database.
    Number,
    /// `w`: the player of the white pieces.
    White,
    /// `b`: the player of the black pieces.
    Black,
    /// `r`: the result.
    Result,
    /// `m`: the length in moves.
    Length,
    /// `d`: the date.
    Date,
    /// `e`: the event.
    Event,
    /// `W`: the Elo of White.
    WhiteElo,
    /// `B`: the Elo of Black.
    BlackElo,
    /// `n`: the round.
    Round,
    /// `s`: the site.
    Site,
    /// `D`: marked for deletion.
    Deleted,
    /// `V`: variations.
    Variations,
    /// `C`: comments.
    Comments,
    /// `A`: annotations (NAGs).
    Annos,
    /// `o`: the ECO code.
    Eco,
    /// `O`: the opening.
    Opening,
    /// `U`: flags.
    Flags,
    /// `S`: the game starts from its own position.
    Start,
    /// `c`: the country of the site.
    Country,
    /// `E`: the date of the event.
    EventDate,
    /// `F`: the material of the final position.
    EndMaterial,
}

/// The side a column's text is aligned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnAlign {
    /// Left (names).
    Left,
    /// Centre.
    Center,
    /// Right (numbers).
    Right,
}

impl ColumnAlign {
    /// The spelling in the preferences file.
    pub fn name(self) -> &'static str {
        match self {
            ColumnAlign::Left => "left",
            ColumnAlign::Center => "center",
            ColumnAlign::Right => "right",
        }
    }

    /// Reads the spelling of [`ColumnAlign::name`].
    pub fn from_name(name: &str) -> Option<ColumnAlign> {
        match name {
            "left" => Some(ColumnAlign::Left),
            "center" => Some(ColumnAlign::Center),
            "right" => Some(ColumnAlign::Right),
            _ => None,
        }
    }

    /// The next alignment in the cycle left, centre, right.
    #[must_use]
    pub fn next(self) -> ColumnAlign {
        match self {
            ColumnAlign::Left => ColumnAlign::Center,
            ColumnAlign::Center => ColumnAlign::Right,
            ColumnAlign::Right => ColumnAlign::Left,
        }
    }
}

/// Which way a sorted list runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortDir {
    /// Smallest first.
    #[default]
    Ascending,
    /// Largest first.
    Descending,
}

impl SortDir {
    /// The other direction.
    #[must_use]
    pub fn flipped(self) -> SortDir {
        match self {
            SortDir::Ascending => SortDir::Descending,
            SortDir::Descending => SortDir::Ascending,
        }
    }
}

impl ListColumn {
    /// Every column, in the order of the Scid (`glistFields`).
    pub const ALL: [ListColumn; 22] = [
        ListColumn::Number,
        ListColumn::White,
        ListColumn::Black,
        ListColumn::Result,
        ListColumn::Length,
        ListColumn::Date,
        ListColumn::Event,
        ListColumn::WhiteElo,
        ListColumn::BlackElo,
        ListColumn::Round,
        ListColumn::Site,
        ListColumn::Deleted,
        ListColumn::Variations,
        ListColumn::Comments,
        ListColumn::Annos,
        ListColumn::Eco,
        ListColumn::Opening,
        ListColumn::Flags,
        ListColumn::Start,
        ListColumn::Country,
        ListColumn::EventDate,
        ListColumn::EndMaterial,
    ];

    /// The name in the preferences file.
    pub fn name(self) -> &'static str {
        match self {
            ListColumn::Number => "number",
            ListColumn::White => "white",
            ListColumn::Black => "black",
            ListColumn::Result => "result",
            ListColumn::Length => "length",
            ListColumn::Date => "date",
            ListColumn::Event => "event",
            ListColumn::WhiteElo => "white_elo",
            ListColumn::BlackElo => "black_elo",
            ListColumn::Round => "round",
            ListColumn::Site => "site",
            ListColumn::Deleted => "deleted",
            ListColumn::Variations => "variations",
            ListColumn::Comments => "comments",
            ListColumn::Annos => "annos",
            ListColumn::Eco => "eco",
            ListColumn::Opening => "opening",
            ListColumn::Flags => "flags",
            ListColumn::Start => "start",
            ListColumn::Country => "country",
            ListColumn::EventDate => "event_date",
            ListColumn::EndMaterial => "end_material",
        }
    }

    /// Reads the spelling of [`ListColumn::name`].
    pub fn from_name(name: &str) -> Option<ListColumn> {
        ListColumn::ALL.into_iter().find(|c| c.name() == name)
    }

    /// The default width in characters (Scid `glistFields`).
    pub fn default_chars(self) -> u16 {
        match self {
            ListColumn::White | ListColumn::Black => 14,
            ListColumn::Result
            | ListColumn::Length
            | ListColumn::WhiteElo
            | ListColumn::BlackElo
            | ListColumn::Round
            | ListColumn::Eco => 5,
            ListColumn::Date | ListColumn::Event | ListColumn::Site => 10,
            ListColumn::Deleted
            | ListColumn::Variations
            | ListColumn::Comments
            | ListColumn::Annos
            | ListColumn::Flags
            | ListColumn::Start
            | ListColumn::Country => 3,
            ListColumn::Opening => 6,
            ListColumn::Number | ListColumn::EventDate | ListColumn::EndMaterial => 7,
        }
    }

    /// The default alignment (Scid: anchor `e` is right, `w` is left).
    pub fn default_align(self) -> ColumnAlign {
        match self {
            ListColumn::White
            | ListColumn::Black
            | ListColumn::Date
            | ListColumn::Event
            | ListColumn::Site
            | ListColumn::Opening
            | ListColumn::EventDate => ColumnAlign::Left,
            _ => ColumnAlign::Right,
        }
    }

    /// Whether clicking the header sorts by this column. The Scid does not
    /// sort by Number, Opening, Flags, Annos, Start or `EndMaterial`.
    pub fn is_sortable(self) -> bool {
        !matches!(
            self,
            ListColumn::Number
                | ListColumn::Opening
                | ListColumn::Flags
                | ListColumn::Annos
                | ListColumn::Start
                | ListColumn::EndMaterial
        )
    }
}

/// The smallest width of a column, in characters.
pub const MIN_COLUMN_CHARS: u16 = 2;
/// The largest width of a column, in characters.
pub const MAX_COLUMN_CHARS: u16 = 60;

/// A column as the user set it up: which one, how it is aligned and how
/// wide it is (in characters, as in the Scid).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct ColumnSetup {
    /// The column.
    pub column: ListColumn,
    /// The alignment.
    pub align: ColumnAlign,
    /// The width in characters, within
    /// [`MIN_COLUMN_CHARS`]..=[`MAX_COLUMN_CHARS`].
    pub chars: u16,
}

impl ColumnSetup {
    /// `column` with its default alignment and width.
    pub fn new(column: ListColumn) -> ColumnSetup {
        ColumnSetup {
            column,
            align: column.default_align(),
            chars: column.default_chars(),
        }
    }

    /// The width limited to the allowed range.
    #[must_use]
    pub fn clamped(self) -> ColumnSetup {
        ColumnSetup {
            chars: self.chars.clamp(MIN_COLUMN_CHARS, MAX_COLUMN_CHARS),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_22_columns_with_unique_names_that_round_trip() {
        assert_eq!(ListColumn::ALL.len(), 22);
        for column in ListColumn::ALL {
            assert_eq!(ListColumn::from_name(column.name()), Some(column));
        }
        let mut names: Vec<_> = ListColumn::ALL.iter().map(|c| c.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 22);
        assert_eq!(ListColumn::from_name("nope"), None);
    }

    #[test]
    fn defaults_follow_glist_fields() {
        assert_eq!(ListColumn::Number.default_chars(), 7);
        assert_eq!(ListColumn::EndMaterial.default_chars(), 7);
        assert_eq!(ListColumn::Opening.default_chars(), 6);
        assert_eq!(ListColumn::White.default_align(), ColumnAlign::Left);
        assert_eq!(ListColumn::Result.default_align(), ColumnAlign::Right);
    }

    #[test]
    fn six_columns_do_not_sort() {
        let unsortable: Vec<_> = ListColumn::ALL
            .into_iter()
            .filter(|c| !c.is_sortable())
            .collect();
        assert_eq!(unsortable.len(), 6);
        assert!(!ListColumn::Number.is_sortable());
        assert!(ListColumn::Country.is_sortable());
    }

    #[test]
    fn widths_are_limited_and_alignment_cycles() {
        let mut setup = ColumnSetup::new(ListColumn::White);
        setup.chars = 500;
        assert_eq!(setup.clamped().chars, MAX_COLUMN_CHARS);
        setup.chars = 0;
        assert_eq!(setup.clamped().chars, MIN_COLUMN_CHARS);
        assert_eq!(ColumnAlign::Left.next().next().next(), ColumnAlign::Left);
        assert_eq!(SortDir::Ascending.flipped(), SortDir::Descending);
    }
}
