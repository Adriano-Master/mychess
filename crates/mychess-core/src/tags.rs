//! PGN header tags: the Seven Tag Roster plus Scid's rating, ECO and event
//! date fields, and free-form extra tags.
//!
//! Limits of the SI4 format (255 bytes per value, 40 extra tags) are not
//! enforced here; they belong to the database layer.

use std::fmt;

use thiserror::Error;

use crate::date::PartialDate;
use crate::eco::Eco;
use crate::error::clip;

/// Highest accepted rating (Scid stores ratings in 12 bits).
pub const MAX_ELO: u16 = 4000;

/// Why a tag or a rating is rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TagError {
    /// The name is empty or has a character other than ASCII letters,
    /// digits and `_`, or the value contains a line break or a NUL.
    #[error("invalid tag name {name:?}")]
    InvalidName {
        /// The offending name, clipped.
        name: String,
    },
    /// The value contains `\r`, `\n` or `\0`.
    #[error("invalid value for tag {name:?}")]
    InvalidValue {
        /// The tag name, clipped.
        name: String,
    },
    /// The name belongs to a dedicated field of [`Tags`] (or is `FEN`).
    /// `White<Type>`/`Black<Type>` are not reserved, see [`Tags::set_extra`].
    #[error("tag {name:?} has a dedicated field and cannot be an extra tag")]
    Reserved {
        /// The offending name, clipped.
        name: String,
    },
    /// The rating is above [`MAX_ELO`].
    #[error("rating {0} is out of range (0..=4000)")]
    Elo(u16),
}

/// A rating value in `0..=4000`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Elo(u16);

impl Elo {
    /// Creates a rating; values above 4000 are an error.
    pub fn new(value: u16) -> Result<Elo, TagError> {
        if value > MAX_ELO {
            Err(TagError::Elo(value))
        } else {
            Ok(Elo(value))
        }
    }

    /// The numeric value.
    pub fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for Elo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The kind of rating, which names the PGN tag (`WhiteElo`, `BlackUSCF`...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[non_exhaustive]
pub enum RatingType {
    /// FIDE Elo (`Elo`), the default.
    #[default]
    Elo,
    /// Generic rating (`Rating`).
    Rating,
    /// Rapid rating (`Rapid`).
    Rapid,
    /// ICCF rating (`ICCF`).
    Iccf,
    /// USCF rating (`USCF`).
    Uscf,
    /// German DWZ (`DWZ`).
    Dwz,
    /// English ECF (`ECF`).
    Ecf,
}

impl RatingType {
    /// All kinds, in Scid's numbering order.
    pub const ALL: [RatingType; 7] = [
        RatingType::Elo,
        RatingType::Rating,
        RatingType::Rapid,
        RatingType::Iccf,
        RatingType::Uscf,
        RatingType::Dwz,
        RatingType::Ecf,
    ];

    /// The name used in tags: `Elo`, `Rating`, `Rapid`, `ICCF`, `USCF`,
    /// `DWZ`, `ECF`.
    pub fn name(self) -> &'static str {
        match self {
            RatingType::Elo => "Elo",
            RatingType::Rating => "Rating",
            RatingType::Rapid => "Rapid",
            RatingType::Iccf => "ICCF",
            RatingType::Uscf => "USCF",
            RatingType::Dwz => "DWZ",
            RatingType::Ecf => "ECF",
        }
    }

    /// Finds a kind by its exact (case-sensitive) tag name.
    pub fn from_name(name: &str) -> Option<RatingType> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

impl fmt::Display for RatingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A player's rating and its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rating {
    /// The rating value.
    pub value: Elo,
    /// The kind of rating.
    pub kind: RatingType,
}

impl Rating {
    /// Creates a rating from a validated value and a kind.
    pub fn new(value: Elo, kind: RatingType) -> Rating {
        Rating { value, kind }
    }
}

/// The result of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum GameResult {
    /// Unknown or in progress, `*`.
    #[default]
    Unknown,
    /// White won, `1-0`.
    White,
    /// Black won, `0-1`.
    Black,
    /// Draw, `1/2-1/2`.
    Draw,
}

impl GameResult {
    /// The PGN result string: `*`, `1-0`, `0-1` or `1/2-1/2`.
    pub fn as_str(self) -> &'static str {
        match self {
            GameResult::Unknown => "*",
            GameResult::White => "1-0",
            GameResult::Black => "0-1",
            GameResult::Draw => "1/2-1/2",
        }
    }

    /// Tolerant reading: text starting with `1-0`, `0-1` or `1/2` (also
    /// `1:0` and `0:1`), after trimming white space. Anything else gives
    /// [`GameResult::Unknown`].
    pub fn from_str_lenient(text: &str) -> GameResult {
        let t = text.trim();
        if t.starts_with("1-0") || t.starts_with("1:0") {
            GameResult::White
        } else if t.starts_with("0-1") || t.starts_with("0:1") {
            GameResult::Black
        } else if t.starts_with("1/2") {
            GameResult::Draw
        } else {
            GameResult::Unknown
        }
    }
}

impl fmt::Display for GameResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The header tags of a game.
///
/// Text fields use `"?"` for "unknown", as in PGN. Tags without a dedicated
/// field are kept as ordered name/value pairs, see [`Tags::set_extra`].
///
/// The public text fields (`event`, `site`, `white`, `black`, `round`) are
/// **not validated**: they may hold quotes, backslashes or line breaks. The
/// PGN writer must sanitise them (replace line breaks with a space) and
/// escape `"` and `\\`. Only [`Tags::set_extra`] validates its input.
///
/// A rating with value 0 (`Some(Rating { value: Elo(0), .. })`) is the same
/// as `None` on output: [`Tags::pgn_pairs`] writes no tag for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tags {
    /// `Event`.
    pub event: String,
    /// `Site`.
    pub site: String,
    /// `White` player name.
    pub white: String,
    /// `Black` player name.
    pub black: String,
    /// `Round`.
    pub round: String,
    /// `Date`.
    pub date: PartialDate,
    /// `EventDate`.
    pub event_date: PartialDate,
    /// `Result`.
    pub result: GameResult,
    /// `White<Type>` rating (`WhiteElo`...).
    pub white_rating: Option<Rating>,
    /// `Black<Type>` rating.
    pub black_rating: Option<Rating>,
    /// `ECO`.
    pub eco: Option<Eco>,
    extra: Vec<(String, String)>,
}

impl Default for Tags {
    /// Scid's defaults (`ClearStandardTags`): `"?"` texts, unknown dates,
    /// result `*`, no ratings, no ECO.
    fn default() -> Tags {
        let unknown = || "?".to_owned();
        Tags {
            event: unknown(),
            site: unknown(),
            white: unknown(),
            black: unknown(),
            round: unknown(),
            date: PartialDate::UNKNOWN,
            event_date: PartialDate::UNKNOWN,
            result: GameResult::Unknown,
            white_rating: None,
            black_rating: None,
            eco: None,
            extra: Vec::new(),
        }
    }
}

/// Whether `name` is written from a dedicated field of [`Tags`] (or is `FEN`,
/// which belongs to the game itself). `White<Type>`/`Black<Type>` are not
/// reserved: Scid keeps only the first rating of each player and turns the
/// others into ordinary tags.
fn is_reserved(name: &str) -> bool {
    const FIXED: [&str; 10] = [
        "Event",
        "Site",
        "Date",
        "Round",
        "White",
        "Black",
        "Result",
        "ECO",
        "EventDate",
        "FEN",
    ];
    FIXED.contains(&name)
}

impl Tags {
    /// Creates tags with the Scid defaults, see [`Tags::default`].
    pub fn new() -> Tags {
        Tags::default()
    }

    /// The tags as written to PGN, in Scid's order: `Event`, `Site`, `Date`,
    /// `Round`, `White`, `Black`, `Result`, then `White<Type>` and
    /// `Black<Type>` (only when the rating is above 0), `ECO` (extended
    /// form), `EventDate` (only when known) and the extra tags in insertion
    /// order. The `FEN` tag is not included: it belongs to the game.
    ///
    /// The first rating type wins: an extra tag whose name equals a rating
    /// pair written here (`WhiteElo` when `white_rating` is of kind `Elo`) is
    /// left out of the output, so no tag appears twice. Extras with other
    /// `White<Type>`/`Black<Type>` names (`WhiteUSCF` next to `WhiteElo`) are
    /// written, as Scid does.
    pub fn pgn_pairs(&self) -> Vec<(String, String)> {
        let pair = |k: &str, v: String| (k.to_owned(), v);
        let mut out = vec![
            pair("Event", self.event.clone()),
            pair("Site", self.site.clone()),
            pair("Date", self.date.to_string()),
            pair("Round", self.round.clone()),
            pair("White", self.white.clone()),
            pair("Black", self.black.clone()),
            pair("Result", self.result.as_str().to_owned()),
        ];
        let mut rating_names = Vec::new();
        for (side, rating) in [("White", self.white_rating), ("Black", self.black_rating)] {
            if let Some(r) = rating.filter(|r| r.value.get() > 0) {
                let name = format!("{side}{}", r.kind);
                rating_names.push(name.clone());
                out.push((name, r.value.to_string()));
            }
        }
        if let Some(eco) = self.eco {
            out.push(pair("ECO", eco.to_string()));
        }
        if !self.event_date.is_unknown() {
            out.push(pair("EventDate", self.event_date.to_string()));
        }
        // The first rating type wins: an extra named like a rating pair
        // written above is dropped from the output (not from `extras()`),
        // so no tag is duplicated.
        out.extend(
            self.extra
                .iter()
                .filter(|(k, _)| !rating_names.contains(k))
                .cloned(),
        );
        out
    }

    /// The extra (non-dedicated) tags in insertion order.
    pub fn extras(&self) -> &[(String, String)] {
        &self.extra
    }

    /// The value of the extra tag `name` (case-sensitive).
    pub fn get_extra(&self, name: &str) -> Option<&str> {
        self.extra
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Sets an extra tag; an existing tag with the same name (case-sensitive)
    /// keeps its position and gets the new value.
    ///
    /// The name must be a PGN symbol: non-empty, only ASCII letters, digits
    /// and `_` (Scid reads a name up to the first space, so anything else
    /// would be lost on a round trip). The value must not contain `\r`, `\n`
    /// or `\0`; quotes and backslashes are allowed (the PGN writer escapes
    /// them). The name must not be one with a dedicated field (`Event`,
    /// `Site`, `Date`, `Round`, `White`, `Black`, `Result`, `ECO`,
    /// `EventDate` and `FEN`). `White<Type>`/`Black<Type>` (`WhiteUSCF`...)
    /// are accepted: Scid keeps only the first rating of each player and
    /// stores the rest as plain tags.
    pub fn set_extra(&mut self, name: &str, value: &str) -> Result<(), TagError> {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(TagError::InvalidName { name: clip(name) });
        }
        if value.contains(['\r', '\n', '\0']) {
            return Err(TagError::InvalidValue { name: clip(name) });
        }
        if is_reserved(name) {
            return Err(TagError::Reserved { name: clip(name) });
        }
        match self.extra.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => value.clone_into(v),
            None => self.extra.push((name.to_owned(), value.to_owned())),
        }
        Ok(())
    }

    /// Removes the extra tag `name`, returning its value.
    pub fn remove_extra(&mut self, name: &str) -> Option<String> {
        let i = self.extra.iter().position(|(k, _)| k == name)?;
        Some(self.extra.remove(i).1)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_scid() {
        let t = Tags::default();
        assert_eq!(t.event, "?");
        assert_eq!(t.round, "?");
        assert_eq!(t.result, GameResult::Unknown);
        assert!(t.date.is_unknown() && t.white_rating.is_none() && t.eco.is_none());
        assert_eq!(t, t.clone());
    }

    #[test]
    fn elo_range() {
        assert_eq!(Elo::new(4000).map(Elo::get), Ok(4000));
        assert_eq!(Elo::new(4001), Err(TagError::Elo(4001)));
        assert_eq!(Elo::new(0).map(Elo::get), Ok(0));
    }

    #[test]
    fn result_lenient() {
        use GameResult::*;
        for (text, r) in [
            ("1-0", White),
            ("0-1", Black),
            ("1/2-1/2", Draw),
            ("1/2", Draw),
            ("1:0", White),
            ("0:1", Black),
            (" 1-0 ", White),
            ("*", Unknown),
            ("", Unknown),
            ("0-0", Unknown),
            ("draw", Unknown),
            ("1-0x", White),
        ] {
            assert_eq!(GameResult::from_str_lenient(text), r, "{text:?}");
        }
        assert_eq!(Draw.to_string(), "1/2-1/2");
        assert_eq!(Unknown.to_string(), "*");
    }

    #[test]
    fn rating_type_names() {
        for t in RatingType::ALL {
            assert_eq!(RatingType::from_name(t.name()), Some(t));
        }
        assert_eq!(RatingType::from_name("elo"), None);
        assert_eq!(RatingType::Iccf.name(), "ICCF");
    }

    #[test]
    fn pgn_pairs_order() {
        let mut t = Tags::default();
        t.event = "Open".into();
        t.white = "A".into();
        t.black = "B".into();
        t.date = "2024.05.17".parse().unwrap();
        t.result = GameResult::Draw;
        t.white_rating = Some(Rating::new(Elo::new(2700).unwrap(), RatingType::Elo));
        t.black_rating = Some(Rating::new(Elo::new(2500).unwrap(), RatingType::Uscf));
        t.eco = Some("B91a".parse().unwrap());
        t.event_date = "2024.05.01".parse().unwrap();
        t.set_extra("Annotator", "me").unwrap();
        t.set_extra("Opening", "Sicilian").unwrap();
        let names: Vec<_> = t
            .pgn_pairs()
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        assert_eq!(
            names,
            [
                "Event=Open",
                "Site=?",
                "Date=2024.05.17",
                "Round=?",
                "White=A",
                "Black=B",
                "Result=1/2-1/2",
                "WhiteElo=2700",
                "BlackUSCF=2500",
                "ECO=B91a",
                "EventDate=2024.05.01",
                "Annotator=me",
                "Opening=Sicilian",
            ]
        );
    }

    #[test]
    fn pgn_pairs_skips_zero_rating_and_unknown_event_date() {
        let mut t = Tags::default();
        t.white_rating = Some(Rating::new(Elo::new(0).unwrap(), RatingType::Elo));
        assert_eq!(t.pgn_pairs().len(), 7);
    }

    #[test]
    fn extra_named_like_an_emitted_rating_is_not_duplicated() {
        let mut t = Tags::default();
        t.white_rating = Some(Rating::new(Elo::new(2700).unwrap(), RatingType::Elo));
        t.set_extra("WhiteElo", "1").unwrap();
        t.set_extra("WhiteUSCF", "2650").unwrap();
        t.set_extra("BlackElo", "2").unwrap();
        let pairs = t.pgn_pairs();
        let count = |n: &str| pairs.iter().filter(|(k, _)| k == n).count();
        assert_eq!(count("WhiteElo"), 1);
        assert_eq!(count("WhiteUSCF"), 1);
        // No black rating was written, so the extra BlackElo is kept.
        assert_eq!(count("BlackElo"), 1);
        assert_eq!(t.extras().len(), 3);
        assert_eq!(pairs[7], ("WhiteElo".to_owned(), "2700".to_owned()));
    }

    #[test]
    fn extras_replace_and_validate() {
        let mut t = Tags::default();
        t.set_extra("A", "1").unwrap();
        t.set_extra("B", "2").unwrap();
        t.set_extra("A", "3").unwrap();
        t.set_extra("a", "4").unwrap();
        assert_eq!(t.extras().len(), 3);
        assert_eq!(t.extras()[0], ("A".to_owned(), "3".to_owned()));
        assert_eq!(t.get_extra("a"), Some("4"));
        assert_eq!(t.remove_extra("B"), Some("2".to_owned()));
        assert_eq!(t.remove_extra("B"), None);
        for bad in ["", "a\"b", "a\nb", "a\rb", "a b", "a[b", "a]b", "é", "a\0b"] {
            assert!(matches!(
                t.set_extra(bad, "x"),
                Err(TagError::InvalidName { .. })
            ));
        }
        for reserved in ["Event", "Site", "ECO", "EventDate", "FEN", "Result"] {
            assert!(matches!(
                t.set_extra(reserved, "x"),
                Err(TagError::Reserved { .. })
            ));
        }
        assert!(t.set_extra("WhiteTitle", "GM").is_ok());
        assert!(t.set_extra("WhiteElo", "2700").is_ok());
        assert!(t.set_extra("BlackECF", "150").is_ok());
        assert!(matches!(
            t.set_extra("k", "a\nb"),
            Err(TagError::InvalidValue { .. })
        ));
        assert!(matches!(
            t.set_extra("k", "a\rb"),
            Err(TagError::InvalidValue { .. })
        ));
        assert!(matches!(
            t.set_extra("k", "a\0b"),
            Err(TagError::InvalidValue { .. })
        ));
        assert!(t.set_extra("Comment_1", "say \"hi\" \\ ok").is_ok());
        assert!(t.set_extra("SetUp", "1").is_ok());
    }
}
