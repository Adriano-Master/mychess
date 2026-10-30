//! Game information: the tags dialog, name suggestions and the information
//! panel (spec M2-2, D133-D141).

mod draft;
mod names;
mod view;

use mychess_core::{GameResult, RatingType, Tags};

use crate::error::RequestId;

pub use draft::{
    DateDraft, ExtraLineError, FieldIssue, FieldIssueKind, IssueSeverity, MAX_EXTRA_TAGS,
    MAX_VALUE_BYTES, RatingDraft, TagField, TagsDraft, Validation, parse_extra_tags_text,
};
pub use names::{MAX_NAME_MATCHES, MAX_NAMES_PER_KIND, NameBook, NameKind, NameMatch, NameSource};
pub(crate) use view::in_variation;
pub use view::{
    COMMENT_PREVIEW_CHARS, GameInfoOptions, GameInfoView, MAX_VARIATION_LINES, NextMove,
    OpeningName, PlayerLine, PreviousMove, VariationLine, game_info,
};

/// Which date of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateTarget {
    /// `Date`.
    Date,
    /// `EventDate`.
    EventDate,
}

/// Which part of a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePart {
    /// The year.
    Year,
    /// The month.
    Month,
    /// The day.
    Day,
}

/// A player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSide {
    /// White.
    White,
    /// Black.
    Black,
}

/// Events of the game information dialog.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GameInfoMessage {
    /// Opens the dialog with the tags of the game being edited.
    Open,
    /// Closes the dialog; nothing is changed.
    Cancel,
    /// Validates and, when nothing blocks, replaces the tags (one undo
    /// step). Warnings do not stop it.
    Apply,
    /// Edits a text field (`White`, `Black`, `Event`, `Site`, `Round`,
    /// `Eco`); other fields are ignored. Updates the name suggestions.
    SetText(TagField, String),
    /// Edits one part of a date.
    SetDate(DateTarget, DatePart, String),
    /// Chooses the result.
    SetResult(GameResult),
    /// Chooses the kind of a rating.
    SetRatingKind(PlayerSide, RatingType),
    /// Edits the value of a rating.
    SetRating(PlayerSide, String),
    /// Edits one extra tag row (rows are added with
    /// [`AddExtra`](GameInfoMessage::AddExtra)).
    SetExtra {
        /// Row number.
        index: usize,
        /// Tag name.
        name: String,
        /// Tag value.
        value: String,
    },
    /// Adds an empty extra tag row.
    AddExtra,
    /// Removes an extra tag row.
    RemoveExtra(usize),
    /// Replaces the extra rows (for the free-text form).
    SetExtras(Vec<(String, String)>),
    /// Asks for today's date (UTC, an effect).
    Today(DateTarget),
    /// Copies the date into the event date ("=Date").
    CopyDateToEventDate,
    /// Fills the ECO field from the opening book.
    ClassifyEco,
    /// Fills the unknown fields from the previous game of the session.
    UsePrevious,
}

/// State of the game information dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct GameInfoDialog {
    /// The fields being edited.
    pub draft: TagsDraft,
    /// The problems found by the last "Apply" or "Classify".
    pub issues: Vec<FieldIssue>,
    /// Name suggestions for the field being edited.
    pub suggestions: Vec<NameMatch>,
    /// The field the suggestions are for.
    pub suggestion_field: Option<TagField>,
    /// The tags of the game opened or saved before this one, for "Use
    /// previous".
    pub last_tags: Option<Tags>,
    /// The "Today" request in flight.
    pub pending_today: Option<(RequestId, DateTarget)>,
}

impl GameInfoDialog {
    /// A dialog on `tags`.
    pub fn new(tags: &Tags, last_tags: Option<Tags>) -> GameInfoDialog {
        GameInfoDialog {
            draft: TagsDraft::from_tags(tags),
            issues: Vec::new(),
            suggestions: Vec::new(),
            suggestion_field: None,
            last_tags,
            pending_today: None,
        }
    }

    fn date_mut(&mut self, target: DateTarget) -> &mut DateDraft {
        match target {
            DateTarget::Date => &mut self.draft.date,
            DateTarget::EventDate => &mut self.draft.event_date,
        }
    }

    fn rating_mut(&mut self, side: PlayerSide) -> &mut RatingDraft {
        match side {
            PlayerSide::White => &mut self.draft.white_rating,
            PlayerSide::Black => &mut self.draft.black_rating,
        }
    }

    /// Checks the draft again and keeps the problems, warnings included,
    /// for the form to show (spec D187: on every edit, not only on OK).
    pub(crate) fn revalidate(&mut self) {
        self.issues = self.draft.validate().issues;
    }

    /// Applies the messages that only edit the draft. Returns `false` for
    /// the others (open, apply, effects), which the controller handles.
    pub(crate) fn edit(&mut self, message: &GameInfoMessage, names: &NameBook) -> bool {
        match message {
            GameInfoMessage::SetText(field, text) => {
                let (slot, kind) = match field {
                    TagField::White => (&mut self.draft.white, Some(NameKind::Player)),
                    TagField::Black => (&mut self.draft.black, Some(NameKind::Player)),
                    TagField::Event => (&mut self.draft.event, Some(NameKind::Event)),
                    TagField::Site => (&mut self.draft.site, Some(NameKind::Site)),
                    TagField::Round => (&mut self.draft.round, Some(NameKind::Round)),
                    TagField::Eco => (&mut self.draft.eco, None),
                    _ => return true,
                };
                text.clone_into(slot);
                match kind {
                    Some(kind) => {
                        self.suggestions = names.matches(kind, text, MAX_NAME_MATCHES);
                        self.suggestion_field = Some(*field);
                    }
                    None => self.clear_suggestions(),
                }
            }
            GameInfoMessage::SetDate(target, part, text) => {
                let date = self.date_mut(*target);
                match part {
                    DatePart::Year => text.clone_into(&mut date.year),
                    DatePart::Month => text.clone_into(&mut date.month),
                    DatePart::Day => text.clone_into(&mut date.day),
                }
            }
            GameInfoMessage::SetResult(result) => self.draft.result = *result,
            GameInfoMessage::SetRatingKind(side, kind) => self.rating_mut(*side).kind = *kind,
            GameInfoMessage::SetRating(side, text) => {
                text.clone_into(&mut self.rating_mut(*side).value);
            }
            GameInfoMessage::SetExtra { index, name, value } => {
                if let Some(row) = self.draft.extra.get_mut(*index) {
                    name.clone_into(&mut row.0);
                    value.clone_into(&mut row.1);
                }
            }
            GameInfoMessage::AddExtra => self.draft.extra.push((String::new(), String::new())),
            GameInfoMessage::RemoveExtra(index) => {
                if *index < self.draft.extra.len() {
                    self.draft.extra.remove(*index);
                }
            }
            GameInfoMessage::SetExtras(rows) => self.draft.extra.clone_from(rows),
            GameInfoMessage::CopyDateToEventDate => {
                self.draft.event_date = self.draft.date.clone();
            }
            _ => return false,
        }
        true
    }

    fn clear_suggestions(&mut self) {
        self.suggestions.clear();
        self.suggestion_field = None;
    }

    /// "Use previous" (D140): fills what is unknown or blank with the
    /// previous game's values.
    pub(crate) fn use_previous(&mut self) {
        let Some(last) = self.last_tags.clone() else {
            return;
        };
        let previous = TagsDraft::from_tags(&last);
        let blank = |s: &str| matches!(s.trim(), "" | "?");
        let d = &mut self.draft;
        for (mine, theirs) in [
            (&mut d.white, previous.white),
            (&mut d.black, previous.black),
            (&mut d.event, previous.event),
            (&mut d.site, previous.site),
            (&mut d.round, previous.round),
            (&mut d.eco, previous.eco),
        ] {
            if blank(mine) {
                *mine = theirs;
            }
        }
        for (mine, theirs) in [
            (&mut d.date, previous.date),
            (&mut d.event_date, previous.event_date),
        ] {
            if mine.year.trim().is_empty()
                && mine.month.trim().is_empty()
                && mine.day.trim().is_empty()
            {
                *mine = theirs;
            }
        }
        for (mine, theirs) in [
            (&mut d.white_rating, previous.white_rating),
            (&mut d.black_rating, previous.black_rating),
        ] {
            if blank(&mine.value) || mine.value.trim() == "0" {
                *mine = theirs;
            }
        }
        for (name, value) in previous.extra {
            if !d.extra.iter().any(|(n, _)| *n == name) {
                d.extra.push((name, value));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> GameInfoDialog {
        GameInfoDialog::new(&Tags::default(), None)
    }

    #[test]
    fn text_edits_update_suggestions() {
        let mut names = NameBook::default();
        names.observe(NameKind::Player, "Carlsen, M.");
        let mut d = dialog();
        assert!(d.edit(
            &GameInfoMessage::SetText(TagField::White, "car".to_owned()),
            &names
        ));
        assert_eq!(d.draft.white, "car");
        assert_eq!(d.suggestions.len(), 1);
        assert_eq!(d.suggestion_field, Some(TagField::White));
        assert!(d.edit(
            &GameInfoMessage::SetText(TagField::Eco, "B90".to_owned()),
            &names
        ));
        assert!(d.suggestions.is_empty());
        assert!(!d.edit(&GameInfoMessage::Apply, &names));
    }

    #[test]
    fn rows_dates_and_ratings_are_edited() {
        let names = NameBook::default();
        let mut d = dialog();
        d.edit(&GameInfoMessage::AddExtra, &names);
        d.edit(
            &GameInfoMessage::SetExtra {
                index: 0,
                name: "A".to_owned(),
                value: "1".to_owned(),
            },
            &names,
        );
        d.edit(
            &GameInfoMessage::SetExtra {
                index: 9,
                name: String::new(),
                value: String::new(),
            },
            &names,
        );
        assert_eq!(d.draft.extra, [("A".to_owned(), "1".to_owned())]);
        d.edit(&GameInfoMessage::RemoveExtra(5), &names);
        d.edit(&GameInfoMessage::RemoveExtra(0), &names);
        assert!(d.draft.extra.is_empty());
        d.edit(
            &GameInfoMessage::SetDate(DateTarget::Date, DatePart::Year, "2024".to_owned()),
            &names,
        );
        d.edit(&GameInfoMessage::CopyDateToEventDate, &names);
        assert_eq!(d.draft.event_date.year, "2024");
        d.edit(
            &GameInfoMessage::SetRating(PlayerSide::Black, "2700".to_owned()),
            &names,
        );
        d.edit(
            &GameInfoMessage::SetRatingKind(PlayerSide::Black, RatingType::Rapid),
            &names,
        );
        assert_eq!(d.draft.black_rating.kind, RatingType::Rapid);
        assert_eq!(d.draft.black_rating.value, "2700");
    }

    #[test]
    fn use_previous_only_fills_blanks() {
        let mut last = Tags::default();
        last.white = "Old".to_owned();
        last.event = "Old event".to_owned();
        last.set_extra("Annotator", "X").expect("extra");
        let mut d = GameInfoDialog::new(&Tags::default(), Some(last));
        d.draft.event = "Mine".to_owned();
        d.use_previous();
        assert_eq!(d.draft.white, "Old");
        assert_eq!(d.draft.event, "Mine");
        assert_eq!(d.draft.extra.len(), 1);
        d.use_previous();
        assert_eq!(d.draft.extra.len(), 1);
    }
}
