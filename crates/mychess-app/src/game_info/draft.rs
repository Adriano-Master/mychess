//! The draft behind the game information dialog and its validation (spec
//! M2-2, D133-D135, D138; Scid: `gameSave -1`, `gsave` in `tcl/end.tcl`).

use mychess_core::{
    Eco, Elo, GameResult, MAX_YEAR, PartialDate, Rating, RatingType, TagError, Tags,
};

/// Longest tag value the SI4 format holds, in bytes (D135).
pub const MAX_VALUE_BYTES: usize = 255;
/// Most extra tags the SI4 format holds.
pub const MAX_EXTRA_TAGS: usize = 40;

/// A date as three texts (year, month, day).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DateDraft {
    /// Year, `????` or empty for unknown.
    pub year: String,
    /// Month, `??` or empty for unknown.
    pub month: String,
    /// Day, `??` or empty for unknown.
    pub day: String,
}

impl DateDraft {
    /// The draft of a date; unknown parts are empty.
    pub fn from_date(date: PartialDate) -> DateDraft {
        DateDraft {
            year: date.year().map(|y| y.to_string()).unwrap_or_default(),
            month: date.month().map(|m| format!("{m:02}")).unwrap_or_default(),
            day: date.day().map(|d| format!("{d:02}")).unwrap_or_default(),
        }
    }
}

/// A rating as a kind and a text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RatingDraft {
    /// The kind (`Elo`, `Rapid`, ...).
    pub kind: RatingType,
    /// The value; empty or `0` means none.
    pub value: String,
}

impl RatingDraft {
    fn from_rating(rating: Option<Rating>) -> RatingDraft {
        rating.map_or_else(RatingDraft::default, |r| RatingDraft {
            kind: r.kind,
            value: r.value.get().to_string(),
        })
    }
}

/// The tags as the user is editing them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagsDraft {
    /// `White`.
    pub white: String,
    /// `Black`.
    pub black: String,
    /// `Event`.
    pub event: String,
    /// `Site`.
    pub site: String,
    /// `Round`.
    pub round: String,
    /// `Date`.
    pub date: DateDraft,
    /// `EventDate`.
    pub event_date: DateDraft,
    /// `Result`.
    pub result: GameResult,
    /// White's rating.
    pub white_rating: RatingDraft,
    /// Black's rating.
    pub black_rating: RatingDraft,
    /// `ECO`.
    pub eco: String,
    /// Extra tags, name and value, in order.
    pub extra: Vec<(String, String)>,
}

/// A field of the dialog, for [`FieldIssue`] and edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TagField {
    /// `White`.
    White,
    /// `Black`.
    Black,
    /// `Event`.
    Event,
    /// `Site`.
    Site,
    /// `Round`.
    Round,
    /// `Date`.
    Date,
    /// `EventDate`.
    EventDate,
    /// `Result`.
    Result,
    /// White's rating.
    WhiteRating,
    /// Black's rating.
    BlackRating,
    /// `ECO`.
    Eco,
    /// The extra tag in this row.
    Extra(usize),
}

/// Whether a problem stops "Apply".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// "Apply" is refused.
    Blocking,
    /// Shown, but "Apply" works.
    Warning,
}

/// What is wrong with a field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldIssueKind {
    /// Year above 2047.
    YearOutOfRange,
    /// Month above 12.
    MonthOutOfRange,
    /// Day above 31.
    DayOutOfRange,
    /// A part is not a number.
    NotANumber,
    /// The day does not exist in that month (kept anyway).
    DayNotInMonth,
    /// `EventDate` is set but `Date` is not.
    EventDateWithoutDate,
    /// Rating above 4000.
    RatingOutOfRange,
    /// Not an ECO code.
    InvalidEco,
    /// No `.eco` file is loaded, so "Classify" cannot work.
    EcoBookNotLoaded,
    /// The value exceeds a limit of the SI4 format.
    ExceedsDatabaseLimit {
        /// The limit (bytes for a value, a count for extra tags).
        max: usize,
    },
    /// The core refused the extra tag.
    ExtraTag(TagError),
    /// The writer controls this tag (`SetUp`, `Variant`, `ScidFlags`).
    DerivedByWriter,
    /// Two rows have the same name; the last one wins.
    DuplicateExtra,
    /// A line break (or NUL) in the value was replaced by a space.
    LineBreakReplaced,
}

impl FieldIssueKind {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            FieldIssueKind::YearOutOfRange => "field-issue-year-out-of-range",
            FieldIssueKind::MonthOutOfRange => "field-issue-month-out-of-range",
            FieldIssueKind::DayOutOfRange => "field-issue-day-out-of-range",
            FieldIssueKind::NotANumber => "field-issue-not-a-number",
            FieldIssueKind::DayNotInMonth => "field-issue-day-not-in-month",
            FieldIssueKind::EventDateWithoutDate => "field-issue-event-date-without-date",
            FieldIssueKind::RatingOutOfRange => "field-issue-rating-out-of-range",
            FieldIssueKind::InvalidEco => "field-issue-invalid-eco",
            FieldIssueKind::EcoBookNotLoaded => "field-issue-eco-book-not-loaded",
            FieldIssueKind::ExceedsDatabaseLimit { .. } => "field-issue-exceeds-database-limit",
            FieldIssueKind::ExtraTag(_) => "field-issue-extra-tag",
            FieldIssueKind::DerivedByWriter => "field-issue-derived-by-writer",
            FieldIssueKind::DuplicateExtra => "field-issue-duplicate-extra",
            FieldIssueKind::LineBreakReplaced => "field-issue-line-break-replaced",
        }
    }
}

/// One problem found by [`TagsDraft::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldIssue {
    /// Where.
    pub field: TagField,
    /// Whether it blocks "Apply".
    pub severity: IssueSeverity,
    /// What.
    pub kind: FieldIssueKind,
}

/// The result of validating a draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validation {
    /// The tags, `None` when some problem is blocking.
    pub tags: Option<Tags>,
    /// Every problem found, blocking or not.
    pub issues: Vec<FieldIssue>,
}

impl Validation {
    /// Whether some problem blocks "Apply".
    pub fn is_blocked(&self) -> bool {
        self.tags.is_none()
    }
}

struct Issues(Vec<FieldIssue>);

impl Issues {
    fn push(&mut self, field: TagField, severity: IssueSeverity, kind: FieldIssueKind) {
        self.0.push(FieldIssue {
            field,
            severity,
            kind,
        });
    }

    fn block(&mut self, field: TagField, kind: FieldIssueKind) {
        self.push(field, IssueSeverity::Blocking, kind);
    }

    fn warn(&mut self, field: TagField, kind: FieldIssueKind) {
        self.push(field, IssueSeverity::Warning, kind);
    }
}

/// A text field: trimmed, line breaks to spaces, empty to `?`.
fn clean_text(raw: &str, field: TagField, issues: &mut Issues) -> String {
    let trimmed = raw.trim();
    let mut value = if trimmed.contains(['\r', '\n', '\0']) {
        issues.warn(field, FieldIssueKind::LineBreakReplaced);
        trimmed.replace(['\r', '\n', '\0'], " ")
    } else {
        trimmed.to_owned()
    };
    if value.is_empty() {
        value.push('?');
    }
    if value.len() > MAX_VALUE_BYTES {
        issues.warn(
            field,
            FieldIssueKind::ExceedsDatabaseLimit {
                max: MAX_VALUE_BYTES,
            },
        );
    }
    value
}

fn days_in_month(year: Option<u16>, month: u8) -> u8 {
    match month {
        2 => match year {
            // Unknown year: the most permissive February.
            Some(y) if !(y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)) => 28,
            _ => 29,
        },
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// One part of a date: `None` for unknown, `Err(())` for a blocking error
/// already reported.
fn date_part(
    text: &str,
    max: u32,
    out_of_range: FieldIssueKind,
    field: TagField,
    issues: &mut Issues,
) -> Result<Option<u32>, ()> {
    let text = text.trim();
    if text.chars().all(|c| c == '?') {
        return Ok(None);
    }
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        issues.block(field, FieldIssueKind::NotANumber);
        return Err(());
    }
    // Digits only: an overflow means "far too large".
    let value = text.parse::<u32>().unwrap_or(u32::MAX);
    if value > max {
        issues.block(field, out_of_range);
        return Err(());
    }
    Ok((value != 0).then_some(value))
}

fn parse_date(draft: &DateDraft, field: TagField, issues: &mut Issues) -> Option<PartialDate> {
    let year = date_part(
        &draft.year,
        u32::from(MAX_YEAR),
        FieldIssueKind::YearOutOfRange,
        field,
        issues,
    );
    let month = date_part(
        &draft.month,
        12,
        FieldIssueKind::MonthOutOfRange,
        field,
        issues,
    );
    let day = date_part(&draft.day, 31, FieldIssueKind::DayOutOfRange, field, issues);
    let (Ok(year), Ok(month), Ok(day)) = (year, month, day) else {
        return None;
    };
    let year = year.and_then(|y| u16::try_from(y).ok());
    let month = month.and_then(|m| u8::try_from(m).ok());
    let day = day.and_then(|d| u8::try_from(d).ok());
    if let (Some(m), Some(d)) = (month, day)
        && d > days_in_month(year, m)
    {
        issues.warn(field, FieldIssueKind::DayNotInMonth);
    }
    PartialDate::new(year, month, day).ok()
}

fn parse_rating(draft: &RatingDraft, field: TagField, issues: &mut Issues) -> Option<Rating> {
    let text = draft.value.trim();
    if text.is_empty() {
        return None;
    }
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        issues.block(field, FieldIssueKind::NotANumber);
        return None;
    }
    let value = text.parse::<u32>().unwrap_or(u32::MAX);
    if value == 0 {
        return None;
    }
    if let Some(Ok(elo)) = u16::try_from(value).ok().map(Elo::new) {
        Some(Rating::new(elo, draft.kind))
    } else {
        issues.block(field, FieldIssueKind::RatingOutOfRange);
        None
    }
}

impl TagsDraft {
    /// The draft of existing tags.
    pub fn from_tags(tags: &Tags) -> TagsDraft {
        TagsDraft {
            white: tags.white.clone(),
            black: tags.black.clone(),
            event: tags.event.clone(),
            site: tags.site.clone(),
            round: tags.round.clone(),
            date: DateDraft::from_date(tags.date),
            event_date: DateDraft::from_date(tags.event_date),
            result: tags.result,
            white_rating: RatingDraft::from_rating(tags.white_rating),
            black_rating: RatingDraft::from_rating(tags.black_rating),
            eco: tags.eco.map(|e| e.to_string()).unwrap_or_default(),
            extra: tags.extras().to_vec(),
        }
    }

    /// Checks every field (D134). The tags are returned when nothing
    /// blocks; warnings are listed either way.
    #[allow(clippy::field_reassign_with_default)] // `Tags` has a private field
    pub fn validate(&self) -> Validation {
        let mut issues = Issues(Vec::new());
        let mut tags = Tags::default();
        tags.white = clean_text(&self.white, TagField::White, &mut issues);
        tags.black = clean_text(&self.black, TagField::Black, &mut issues);
        tags.event = clean_text(&self.event, TagField::Event, &mut issues);
        tags.site = clean_text(&self.site, TagField::Site, &mut issues);
        tags.round = clean_text(&self.round, TagField::Round, &mut issues);
        tags.result = self.result;

        let date = parse_date(&self.date, TagField::Date, &mut issues);
        let event_date = parse_date(&self.event_date, TagField::EventDate, &mut issues);
        if let (Some(date), Some(event_date)) = (date, event_date) {
            tags.date = date;
            tags.event_date = event_date;
            if date.is_unknown() && !event_date.is_unknown() {
                issues.block(TagField::EventDate, FieldIssueKind::EventDateWithoutDate);
            }
        }
        tags.white_rating = parse_rating(&self.white_rating, TagField::WhiteRating, &mut issues);
        tags.black_rating = parse_rating(&self.black_rating, TagField::BlackRating, &mut issues);

        let eco = self.eco.trim();
        if !eco.is_empty() {
            match eco.parse::<Eco>() {
                Ok(code) => tags.eco = Some(code),
                Err(_) => issues.block(TagField::Eco, FieldIssueKind::InvalidEco),
            }
        }
        self.validate_extras(&mut tags, &mut issues);

        let blocked = issues
            .0
            .iter()
            .any(|i| i.severity == IssueSeverity::Blocking);
        Validation {
            tags: (!blocked).then_some(tags),
            issues: issues.0,
        }
    }

    fn validate_extras(&self, tags: &mut Tags, issues: &mut Issues) {
        let mut seen: Vec<&str> = Vec::new();
        let mut rows = 0usize;
        for (row, (name, value)) in self.extra.iter().enumerate() {
            let name = name.trim();
            if name.is_empty() && value.trim().is_empty() {
                continue;
            }
            rows += 1;
            let field = TagField::Extra(row);
            if let Err(error) = tags.set_extra(name, value) {
                issues.block(field, FieldIssueKind::ExtraTag(error));
                continue;
            }
            if matches!(name, "SetUp" | "Variant" | "ScidFlags") {
                issues.warn(field, FieldIssueKind::DerivedByWriter);
            }
            if seen.contains(&name) {
                issues.warn(field, FieldIssueKind::DuplicateExtra);
            }
            seen.push(name);
            if value.len() > MAX_VALUE_BYTES {
                issues.warn(
                    field,
                    FieldIssueKind::ExceedsDatabaseLimit {
                        max: MAX_VALUE_BYTES,
                    },
                );
            }
            if rows == MAX_EXTRA_TAGS + 1 {
                issues.warn(
                    field,
                    FieldIssueKind::ExceedsDatabaseLimit {
                        max: MAX_EXTRA_TAGS,
                    },
                );
            }
        }
    }
}

/// A line of the extra tags text that is not `Name "value"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraLineError {
    /// Line number, 1-based.
    pub line: usize,
    /// The line, clipped to 80 characters.
    pub text: String,
}

/// Reads the Scid's extra tags text: one `Name "value"` per line, blank
/// lines ignored. Unlike the Scid, a bad line is reported, not skipped.
///
/// # Errors
///
/// Every line that does not have the form `Name "value"`.
pub fn parse_extra_tags_text(text: &str) -> Result<Vec<(String, String)>, Vec<ExtraLineError>> {
    let mut pairs = Vec::new();
    let mut errors = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed = line
            .split_once(char::is_whitespace)
            .and_then(|(name, rest)| {
                let rest = rest.trim();
                let value = rest.strip_prefix('"')?.strip_suffix('"')?;
                (!name.is_empty() && !name.contains('"'))
                    .then(|| (name.to_owned(), value.to_owned()))
            });
        match parsed {
            Some(pair) => pairs.push(pair),
            None => errors.push(ExtraLineError {
                line: index + 1,
                text: line.chars().take(80).collect(),
            }),
        }
    }
    if errors.is_empty() {
        Ok(pairs)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use super::*;

    fn draft() -> TagsDraft {
        TagsDraft::from_tags(&Tags::default())
    }

    fn kinds(v: &Validation) -> Vec<FieldIssueKind> {
        v.issues.iter().map(|i| i.kind.clone()).collect()
    }

    fn date(y: &str, m: &str, d: &str) -> DateDraft {
        DateDraft {
            year: y.to_owned(),
            month: m.to_owned(),
            day: d.to_owned(),
        }
    }

    #[test]
    fn default_tags_round_trip() {
        let v = TagsDraft::from_tags(&Tags::default()).validate();
        assert_eq!(v.tags, Some(Tags::default()));
        assert!(v.issues.is_empty());
        let mut tags = Tags::default();
        tags.white = "Carlsen, M.".to_owned();
        tags.date = PartialDate::new(Some(2024), Some(5), None).expect("date");
        tags.white_rating = Some(Rating::new(Elo::new(2850).expect("elo"), RatingType::Rapid));
        tags.eco = "B91a".parse().ok();
        tags.set_extra("Annotator", "Anand").expect("extra");
        let v = TagsDraft::from_tags(&tags).validate();
        assert_eq!(v.tags, Some(tags));
    }

    #[test]
    fn text_fields_are_cleaned() {
        let mut d = draft();
        d.white = "  ".to_owned();
        d.black = " Carlsen, M. ".to_owned();
        d.event = "a\nb".to_owned();
        d.site = "x".repeat(300);
        let v = d.validate();
        let tags = v.tags.clone().expect("applicable");
        assert_eq!(tags.white, "?");
        assert_eq!(tags.black, "Carlsen, M.");
        assert_eq!(tags.event, "a b");
        assert!(kinds(&v).contains(&FieldIssueKind::LineBreakReplaced));
        assert!(kinds(&v).contains(&FieldIssueKind::ExceedsDatabaseLimit { max: 255 }));
    }

    #[test]
    fn dates_follow_the_rules() {
        let check = |y: &str, m: &str, d: &str| {
            let mut dr = draft();
            dr.date = date(y, m, d);
            dr.validate()
        };
        let ok = check("2024", "05", "17");
        assert_eq!(
            ok.tags.map(|t| t.date.to_string()),
            Some("2024.05.17".to_owned())
        );
        for (y, m, d) in [("????", "??", "??"), ("", "", ""), ("0", "0", "0")] {
            let v = check(y, m, d);
            assert!(v.tags.expect("ok").date.is_unknown(), "{y}{m}{d}");
        }
        assert!(kinds(&check("2048", "", "")).contains(&FieldIssueKind::YearOutOfRange));
        assert!(kinds(&check("2024", "13", "")).contains(&FieldIssueKind::MonthOutOfRange));
        assert!(kinds(&check("2024", "01", "32")).contains(&FieldIssueKind::DayOutOfRange));
        assert!(kinds(&check("abc", "", "")).contains(&FieldIssueKind::NotANumber));
        let feb = check("2024", "02", "30");
        assert!(feb.tags.is_some());
        assert!(kinds(&feb).contains(&FieldIssueKind::DayNotInMonth));
        assert_eq!(check("2023", "02", "29").issues.len(), 1);
        assert!(check("2024", "02", "29").issues.is_empty());
        assert!(check("", "", "15").tags.is_some());
    }

    #[test]
    fn event_date_needs_a_date() {
        let mut d = draft();
        d.event_date = date("2020", "", "");
        let v = d.validate();
        assert!(v.is_blocked());
        assert_eq!(v.issues[0].field, TagField::EventDate);
        d.date = date("2020", "", "");
        assert!(d.validate().tags.is_some());
    }

    #[test]
    fn ratings() {
        let mut d = draft();
        d.white_rating = RatingDraft {
            kind: RatingType::Elo,
            value: "0".to_owned(),
        };
        d.black_rating = RatingDraft {
            kind: RatingType::Uscf,
            value: String::new(),
        };
        let tags = d.validate().tags.expect("ok");
        assert!(tags.white_rating.is_none() && tags.black_rating.is_none());
        d.white_rating.value = "2850".to_owned();
        assert_eq!(
            d.validate()
                .tags
                .expect("ok")
                .white_rating
                .map(|r| r.value.get()),
            Some(2850)
        );
        d.white_rating.value = "4001".to_owned();
        assert!(kinds(&d.validate()).contains(&FieldIssueKind::RatingOutOfRange));
        d.white_rating.value = "28x".to_owned();
        assert!(kinds(&d.validate()).contains(&FieldIssueKind::NotANumber));
    }

    #[test]
    fn eco_codes() {
        let mut d = draft();
        for (text, ok) in [
            ("", true),
            ("b91a", true),
            ("C1", true),
            ("E99b2", true),
            ("F00", false),
            ("B91zz", false),
        ] {
            d.eco = text.to_owned();
            assert_eq!(d.validate().tags.is_some(), ok, "{text:?}");
        }
        d.eco = "B9".to_owned();
        // The core reads partial codes like `C1` (= C10), so `B9` is B90.
        assert!(d.validate().tags.is_some());
    }

    #[test]
    fn extra_tags() {
        let mut d = draft();
        d.extra = vec![
            ("Annotator".to_owned(), "Anand".to_owned()),
            (String::new(), String::new()),
        ];
        assert!(d.validate().tags.is_some());
        for (name, value, blocking) in [
            ("Bad Name", "x", true),
            ("White", "x", true),
            ("FEN", "x", true),
            ("Note", "a\nb", true),
            ("SetUp", "1", false),
        ] {
            d.extra = vec![(name.to_owned(), value.to_owned())];
            let v = d.validate();
            assert_eq!(v.is_blocked(), blocking, "{name}");
            assert_eq!(v.issues.len(), 1, "{name}");
        }
        d.extra = vec![
            ("A".to_owned(), "1".to_owned()),
            ("A".to_owned(), "2".to_owned()),
        ];
        let v = d.validate();
        assert!(kinds(&v).contains(&FieldIssueKind::DuplicateExtra));
        assert_eq!(v.tags.expect("ok").get_extra("A"), Some("2"));
        d.extra = (0..41).map(|i| (format!("T{i}"), "v".to_owned())).collect();
        assert!(kinds(&d.validate()).contains(&FieldIssueKind::ExceedsDatabaseLimit { max: 40 }));
    }

    #[test]
    fn extra_text_parsing_reports_bad_lines() {
        let ok = parse_extra_tags_text("Annotator \"Anand\"\n\nRound2 \"x y\"");
        assert_eq!(
            ok,
            Ok(vec![
                ("Annotator".to_owned(), "Anand".to_owned()),
                ("Round2".to_owned(), "x y".to_owned())
            ])
        );
        let err = parse_extra_tags_text("A \"x\"\nAnnotator Anand").expect_err("bad line");
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].line, 2);
    }
}
