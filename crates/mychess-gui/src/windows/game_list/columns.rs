//! The columns of the game list: the 22 columns of the Scid, how they print
//! and how the user edits them (spec M3-4c, D562).

use iced::alignment;
use mychess_app::{ColumnAlign, ColumnSetup, GameListPrefs, GameRow, ListColumn};
use mychess_core::GameResult;

use super::ELO_MIN_WIDTH;

/// A column of the list: the 22 columns of the Scid (`glistFields`). The
/// application keeps which are shown and how ([`ListColumn`] in the
/// preferences); this type adds what the screen needs: the header text, the
/// cell text and the iced alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    /// The number of the game in the database.
    Number,
    /// The player of the white pieces.
    White,
    /// The player of the black pieces.
    Black,
    /// The result.
    Result,
    /// The length in moves.
    Length,
    /// The date.
    Date,
    /// The event.
    Event,
    /// The Elo of White.
    WhiteElo,
    /// The Elo of Black.
    BlackElo,
    /// The round.
    Round,
    /// The site.
    Site,
    /// The game is marked for deletion.
    Deleted,
    /// Variations.
    Variations,
    /// Comments.
    Comments,
    /// Annotations (NAGs).
    Annos,
    /// The ECO code.
    Eco,
    /// The opening.
    Opening,
    /// Flags.
    Flags,
    /// The game starts from its own position.
    Start,
    /// The country of the site.
    Country,
    /// The date of the event.
    EventDate,
    /// The material of the final position.
    EndMaterial,
}

impl Column {
    /// The columns every width shows by default, in the order of the Scid
    /// `glistFields` (D562).
    pub const BASIC: [Column; 8] = [
        Column::Number,
        Column::White,
        Column::Black,
        Column::Result,
        Column::Length,
        Column::Date,
        Column::Event,
        Column::Eco,
    ];

    /// The basic columns and the two Elo columns, for a wide panel.
    pub const WITH_ELO: [Column; 10] = [
        Column::Number,
        Column::White,
        Column::Black,
        Column::Result,
        Column::Length,
        Column::Date,
        Column::Event,
        Column::WhiteElo,
        Column::BlackElo,
        Column::Eco,
    ];

    /// The column of the application's setup.
    pub fn of(column: ListColumn) -> Column {
        match column {
            ListColumn::Number => Column::Number,
            ListColumn::White => Column::White,
            ListColumn::Black => Column::Black,
            ListColumn::Result => Column::Result,
            ListColumn::Length => Column::Length,
            ListColumn::Date => Column::Date,
            ListColumn::Event => Column::Event,
            ListColumn::WhiteElo => Column::WhiteElo,
            ListColumn::BlackElo => Column::BlackElo,
            ListColumn::Round => Column::Round,
            ListColumn::Site => Column::Site,
            ListColumn::Deleted => Column::Deleted,
            ListColumn::Variations => Column::Variations,
            ListColumn::Comments => Column::Comments,
            ListColumn::Annos => Column::Annos,
            ListColumn::Eco => Column::Eco,
            ListColumn::Opening => Column::Opening,
            ListColumn::Flags => Column::Flags,
            ListColumn::Start => Column::Start,
            ListColumn::Country => Column::Country,
            ListColumn::EventDate => Column::EventDate,
            _ => Column::EndMaterial,
        }
    }

    /// The column in the application's terms.
    pub fn list_column(self) -> ListColumn {
        match self {
            Column::Number => ListColumn::Number,
            Column::White => ListColumn::White,
            Column::Black => ListColumn::Black,
            Column::Result => ListColumn::Result,
            Column::Length => ListColumn::Length,
            Column::Date => ListColumn::Date,
            Column::Event => ListColumn::Event,
            Column::WhiteElo => ListColumn::WhiteElo,
            Column::BlackElo => ListColumn::BlackElo,
            Column::Round => ListColumn::Round,
            Column::Site => ListColumn::Site,
            Column::Deleted => ListColumn::Deleted,
            Column::Variations => ListColumn::Variations,
            Column::Comments => ListColumn::Comments,
            Column::Annos => ListColumn::Annos,
            Column::Eco => ListColumn::Eco,
            Column::Opening => ListColumn::Opening,
            Column::Flags => ListColumn::Flags,
            Column::Start => ListColumn::Start,
            Column::Country => ListColumn::Country,
            Column::EventDate => ListColumn::EventDate,
            Column::EndMaterial => ListColumn::EndMaterial,
        }
    }

    /// The default width in characters, as in the Scid (`glistFields`).
    pub fn chars(self) -> u16 {
        self.list_column().default_chars()
    }

    /// The default alignment, as in the Scid: numbers to the right, names
    /// to the left.
    pub fn align(self) -> alignment::Horizontal {
        horizontal(self.list_column().default_align())
    }

    /// The text of the cell of `row`.
    pub fn cell(self, row: &GameRow) -> String {
        let count = |n: u8| if n == 0 { String::new() } else { n.to_string() };
        match self {
            Column::Number => row.number.to_string(),
            Column::White => row.white.clone(),
            Column::Black => row.black.clone(),
            Column::Result => result_text(row.result).to_owned(),
            Column::Date => row.date.clone(),
            Column::Event => row.event.clone(),
            Column::Eco => row.eco.clone(),
            Column::Length => row.length.to_string(),
            Column::WhiteElo => elo_text(row.white_elo),
            Column::BlackElo => elo_text(row.black_elo),
            Column::Round => row.round.clone(),
            Column::Site => row.site.clone(),
            Column::Deleted => if row.deleted { "D" } else { "" }.to_owned(),
            Column::Variations => count(row.variations),
            Column::Comments => count(row.comments),
            Column::Annos => count(row.annos),
            Column::Opening => row.opening.clone(),
            Column::Flags => row.flags.clone(),
            Column::Start => if row.start { "S" } else { "" }.to_owned(),
            Column::Country => row.country.clone(),
            Column::EventDate => row.event_date.clone(),
            Column::EndMaterial => row.end_material.clone(),
        }
    }
}

/// The iced alignment of a column alignment of the preferences.
pub(super) fn horizontal(align: ColumnAlign) -> alignment::Horizontal {
    match align {
        ColumnAlign::Left => alignment::Horizontal::Left,
        ColumnAlign::Center => alignment::Horizontal::Center,
        ColumnAlign::Right => alignment::Horizontal::Right,
    }
}

/// The result as the Scid prints it in the list.
pub(super) fn result_text(result: GameResult) -> &'static str {
    match result {
        GameResult::White => "1-0",
        GameResult::Black => "0-1",
        GameResult::Draw => "=-=",
        _ => "*",
    }
}

/// An Elo, blank when the game has none.
pub(super) fn elo_text(elo: u16) -> String {
    if elo == 0 {
        String::new()
    } else {
        elo.to_string()
    }
}

/// The default columns that fit a panel of `width` logical pixels: the two
/// Elo columns need `ELO_MIN_WIDTH`.
pub fn columns_for_width(width: f32) -> &'static [Column] {
    if width >= ELO_MIN_WIDTH {
        &Column::WITH_ELO
    } else {
        &Column::BASIC
    }
}

/// The columns of `setups` that a panel of `width` logical pixels shows:
/// the Elo columns drop out below `ELO_MIN_WIDTH`, the other columns of
/// the user's setup are always shown (the rows are clipped, not wrapped).
pub fn visible_setups(setups: &[ColumnSetup], width: f32) -> Vec<ColumnSetup> {
    setups
        .iter()
        .filter(|s| {
            width >= ELO_MIN_WIDTH
                || !matches!(s.column, ListColumn::WhiteElo | ListColumn::BlackElo)
        })
        .copied()
        .collect()
}

/// A change to the columns, saved in the preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnEdit {
    /// Show the column, or hide it (the last one stays).
    Toggle(Column),
    /// Align the column.
    Align(Column, ColumnAlign),
    /// Two characters wider.
    Wider(Column),
    /// Two characters narrower.
    Narrower(Column),
    /// One place to the left.
    MoveLeft(Column),
    /// One place to the right.
    MoveRight(Column),
    /// The default columns.
    Reset,
}

/// The columns after `edit`.
pub fn edit_columns(prefs: &GameListPrefs, edit: &ColumnEdit) -> GameListPrefs {
    let mut columns = prefs.columns.clone();
    let index = |column: Column| {
        columns
            .iter()
            .position(|c| c.column == column.list_column())
    };
    match edit {
        ColumnEdit::Reset => return GameListPrefs::default(),
        ColumnEdit::Toggle(column) => match index(*column) {
            Some(at) if columns.len() > 1 => {
                columns.remove(at);
            }
            Some(_) => {}
            None => columns.push(ColumnSetup::new(column.list_column())),
        },
        ColumnEdit::Align(column, align) => {
            if let Some(at) = index(*column)
                && let Some(setup) = columns.get_mut(at)
            {
                setup.align = *align;
            }
        }
        ColumnEdit::Wider(column) | ColumnEdit::Narrower(column) => {
            if let Some(at) = index(*column)
                && let Some(setup) = columns.get_mut(at)
            {
                setup.chars = if matches!(edit, ColumnEdit::Wider(_)) {
                    setup.chars.saturating_add(2)
                } else {
                    setup.chars.saturating_sub(2)
                };
            }
        }
        ColumnEdit::MoveLeft(column) => {
            if let Some(at) = index(*column).filter(|at| *at > 0) {
                columns.swap(at, at - 1);
            }
        }
        ColumnEdit::MoveRight(column) => {
            if let Some(at) = index(*column).filter(|at| at + 1 < columns.len()) {
                columns.swap(at, at + 1);
            }
        }
    }
    GameListPrefs { columns }.sanitized()
}

impl Column {
    /// The Fluent key of the header.
    pub fn label_key(self) -> &'static str {
        match self {
            Column::Number => "games-col-number",
            Column::White => "games-col-white",
            Column::Black => "games-col-black",
            Column::Result => "games-col-result",
            Column::Date => "games-col-date",
            Column::Event => "games-col-event",
            Column::Eco => "games-col-eco",
            Column::Length => "games-col-length",
            Column::WhiteElo => "games-col-white-elo",
            Column::BlackElo => "games-col-black-elo",
            Column::Round => "games-col-round",
            Column::Site => "games-col-site",
            Column::Deleted => "games-col-deleted",
            Column::Variations => "games-col-variations",
            Column::Comments => "games-col-comments",
            Column::Annos => "games-col-annos",
            Column::Opening => "games-col-opening",
            Column::Flags => "games-col-flags",
            Column::Start => "games-col-start",
            Column::Country => "games-col-country",
            Column::EventDate => "games-col-event-date",
            Column::EndMaterial => "games-col-end-material",
        }
    }
}
