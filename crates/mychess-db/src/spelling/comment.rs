// Derived from Scid vs PC 4.27, src/spellchk.cpp (`GetTitle`,
// `GetLastCountry`, `GetPeakRating`, `GetBirthdate`, `GetDeathdate`).
// Copyright (c) 2000-2003 Shane Hudson; Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The comment of a player in the spelling file
//! (`docs/formatos/spelling-ssp.md`, section 3; rules 16 to 24).

use mychess_core::PartialDate;

use crate::search::Title;

/// The titles `GetTitle` of the Scid looks for, in its order.
const TABLE_TITLES: [&str; 10] = [
    "gm", "im", "fm", "cm", "wgm", "wim", "wfm", "cgm", "cim", "hgm",
];

/// A decoded player comment: `TITLES COUNTRIES [PEAK] BIRTH[--DEATH] FLAGS`.
///
/// Every field that is missing is empty or unknown; nothing fails (rules 23
/// and 24). The sign-posts `i` and `w` of the fifth field are ignored.
///
/// ```
/// use mychess_db::spelling::PlayerComment;
///
/// let c = PlayerComment::parse("cim URS/RUS [2520*] 1911.06.14--2004.02.29");
/// assert_eq!(c.last_country(), "RUS");
/// assert_eq!(c.peak(), 2520);
/// assert!(c.peak_is_estimate());
/// assert_eq!(c.died().to_string(), "2004.02.29");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerComment<'a> {
    titles: &'a str,
    countries: &'a str,
    born_field: &'a str,
    peak: u16,
    peak_estimate: bool,
    born: PartialDate,
    died: PartialDate,
}

impl<'a> PlayerComment<'a> {
    /// Decodes a comment.
    pub fn parse(text: &'a str) -> PlayerComment<'a> {
        let mut fields = text.split_ascii_whitespace();
        let titles = fields.next().unwrap_or("");
        let countries = fields.next().unwrap_or("");
        let born_field = fields.nth(1).unwrap_or("");
        let (peak, peak_estimate) = peak_of(text);
        let after = after_bracket(text);
        let born = after.map_or(PartialDate::UNKNOWN, date_token);
        let died = after.map_or(PartialDate::UNKNOWN, |rest| {
            let rest = rest.trim_start_matches(' ');
            let from_dash = rest.find('-').and_then(|i| rest.get(i..));
            from_dash.map_or(PartialDate::UNKNOWN, |s| {
                date_token(s.trim_start_matches('-'))
            })
        });
        PlayerComment {
            titles,
            countries,
            born_field,
            peak,
            peak_estimate,
            born,
            died,
        }
    }

    /// The raw title field (`GM`, `im+cim`, `-`, empty).
    pub fn title_field(&self) -> &'a str {
        self.titles
    }

    /// The title codes, as written, without `-` and empty parts.
    pub fn titles_raw(&self) -> Vec<&'a str> {
        self.titles
            .split('+')
            .filter(|t| !t.is_empty() && *t != "-")
            .collect()
    }

    /// The title for the header search (rule 18): the first code, compared
    /// without regard to case.
    pub fn title(&self) -> Option<Title> {
        Title::from_scid_str(self.titles.split('+').next().unwrap_or(""))
    }

    /// The first code if it is one of the titles `GetTitle` of the Scid
    /// knows (lowercase), for the tournament table.
    pub fn title_for_table(&self) -> Option<&'static str> {
        let first = self.titles.split('+').next().unwrap_or("");
        TABLE_TITLES
            .iter()
            .copied()
            .find(|t| t.eq_ignore_ascii_case(first))
    }

    /// The raw country field (`ENG/NZL`, `-`, empty).
    pub fn country_field(&self) -> &'a str {
        self.countries
    }

    /// The country codes, oldest first, without `-` and empty parts.
    pub fn countries(&self) -> Vec<&'a str> {
        self.countries
            .split('/')
            .filter(|c| !c.is_empty() && *c != "-")
            .collect()
    }

    /// The current country (rule 19): the last three characters of the
    /// field, or empty if it has fewer than three.
    pub fn last_country(&self) -> &'a str {
        let field = self.countries;
        field
            .len()
            .checked_sub(3)
            .and_then(|start| field.get(start..))
            .unwrap_or("")
    }

    /// The peak rating inside the brackets; 0 if unknown (`[....]`).
    pub fn peak(&self) -> u16 {
        self.peak
    }

    /// Whether the peak ends in `*` (estimate for a historic player).
    pub fn peak_is_estimate(&self) -> bool {
        self.peak_estimate
    }

    /// The fourth field as written (`1904.??.??--1979.08.15`, `????`), the
    /// text the information window shows after `Born:`.
    pub fn born_field(&self) -> &'a str {
        self.born_field
    }

    /// The birth date; unknown parts are `None`.
    pub fn born(&self) -> PartialDate {
        self.born
    }

    /// The death date (after `--`).
    pub fn died(&self) -> PartialDate {
        self.died
    }
}

/// The text after the first `]`, if there is one.
fn after_bracket(text: &str) -> Option<&str> {
    let i = text.find(']')?;
    text.get(i + 1..)
}

/// The integer after the first `[` and whether a `*` follows it.
fn peak_of(text: &str) -> (u16, bool) {
    let Some(rest) = text.find('[').and_then(|i| text.get(i + 1..)) else {
        return (0, false);
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let value = rest
        .get(..digits)
        .and_then(|d| d.parse::<u32>().ok())
        .unwrap_or(if digits > 0 { u32::MAX } else { 0 });
    let estimate = digits > 0 && rest.as_bytes().get(digits) == Some(&b'*');
    (u16::try_from(value).unwrap_or(u16::MAX), estimate)
}

/// A date at the start of `text`, up to the first white space.
/// The date at the start of `text`, up to the first blank or `-`. The Scid
/// (`date.cpp`, `date_EncodeFromString`) does not stop at the `-` that
/// separates birth from death, so `1904--1979.08.15` gives it a birth of
/// `1904.08.15`; the grammar of `docs/formatos/spelling-ssp.md` (section 3)
/// says the birth is `1904` alone, and that is what is kept (D3359). The real
/// file always writes the full `????.??.??--...` form, so nothing changes
/// for it.
fn date_token(text: &str) -> PartialDate {
    let text = text.trim_start_matches(' ');
    let end = text
        .find(|c: char| c.is_ascii_whitespace() || c == '-')
        .unwrap_or(text.len());
    PartialDate::parse_lenient(text.get(..end).unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(text: &str) -> String {
        text.to_owned()
    }

    #[test]
    fn the_seven_real_examples() {
        let cases = [
            (
                "GM RUS [2812] 1963.04.13 i",
                "RUS",
                2812,
                false,
                "1963.04.13",
                "????.??.??",
            ),
            (
                "- BAN [1648] 2015",
                "BAN",
                1648,
                false,
                "2015.??.??",
                "????.??.??",
            ),
            (
                "- ENG/NZL [....] 1904.??.??--1979.08.15",
                "NZL",
                0,
                false,
                "1904.??.??",
                "1979.08.15",
            ),
            (
                "cim URS/RUS [2520*] 1911.06.14--2004.02.29",
                "RUS",
                2520,
                true,
                "1911.06.14",
                "2004.02.29",
            ),
            (
                "- FRA [....] ????",
                "FRA",
                0,
                false,
                "????.??.??",
                "????.??.??",
            ),
            (
                "- AUS [1490]",
                "AUS",
                1490,
                false,
                "????.??.??",
                "????.??.??",
            ),
            (
                "w KAZ [2185] ????",
                "KAZ",
                2185,
                false,
                "????.??.??",
                "????.??.??",
            ),
        ];
        for (text, country, peak, est, born, died) in cases {
            let c = PlayerComment::parse(text);
            assert_eq!(c.last_country(), country, "{text}");
            assert_eq!(c.peak(), peak, "{text}");
            assert_eq!(c.peak_is_estimate(), est, "{text}");
            assert_eq!(c.born().to_string(), d(born), "{text}");
            assert_eq!(c.died().to_string(), d(died), "{text}");
        }
    }

    #[test]
    fn death_with_only_a_year_or_partial_parts() {
        let c = PlayerComment::parse("- X [....] ????.??.??--1977.02.??");
        assert_eq!(c.died().to_string(), "1977.02.??");
        assert!(c.born().is_unknown());
        let c = PlayerComment::parse("- X [1] 1900--1979");
        assert_eq!(c.died().year(), Some(1979));
    }

    #[test]
    fn titles_in_any_case() {
        for t in ["GM", "gm", "Gm", "hgm", "HGM"] {
            let text = format!("{t} RUS [2800] 1963");
            let c = PlayerComment::parse(&text);
            assert_eq!(c.title(), Some(Title::Gm), "{t}");
        }
        for t in ["cim", "csm", "w", "-"] {
            let text = format!("{t} RUS [2800] 1963");
            let c = PlayerComment::parse(&text);
            assert_eq!(c.title(), None, "{t}");
        }
        let c = PlayerComment::parse("im+cim URS [2500] 1900");
        assert_eq!(c.title(), Some(Title::Im));
        assert_eq!(c.titles_raw(), ["im", "cim"]);
        assert_eq!(c.title_for_table(), Some("im"));
        assert_eq!(PlayerComment::parse("- A [1]").titles_raw().len(), 0);
    }

    #[test]
    fn malformed_and_empty_comments_never_fail() {
        for text in [
            "",
            " ",
            "-",
            "[",
            "]",
            "[abc",
            "- AUS",
            "x y [99999999999999] ??",
            "é é",
        ] {
            let c = PlayerComment::parse(text);
            let _ = (c.last_country(), c.peak(), c.born(), c.died());
        }
        let c = PlayerComment::parse("");
        assert_eq!(c.last_country(), "");
        assert_eq!(c.peak(), 0);
        assert!(c.born().is_unknown());
        assert_eq!(PlayerComment::parse("- AU [1]").last_country(), "");
        assert_eq!(
            PlayerComment::parse("x y [99999999999999]").peak(),
            u16::MAX
        );
    }

    #[test]
    fn countries_list() {
        let c = PlayerComment::parse("- YUG/SCG/SRB [1] 1950");
        assert_eq!(c.countries(), ["YUG", "SCG", "SRB"]);
        assert_eq!(c.last_country(), "SRB");
        assert_eq!(PlayerComment::parse("- - [1]").countries().len(), 0);
    }
}
