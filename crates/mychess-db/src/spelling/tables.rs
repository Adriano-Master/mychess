// Derived from Scid vs PC 4.27, src/spellchk.cpp (`titleTable`,
// `countryTable`). Copyright (c) 2000-2003 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The title and country tables of the player information window
//! (`docs/specs/m6-3-jogadores-torneios-spelling.md`, rules 17 and 19).

/// Titles of the `titleTable` of Scid: `(code, English name)`, lowercase codes.
pub const TITLE_TABLE: &[(&str, &str)] = &[
    ("gm", "Grandmaster"),
    ("im", "International Master"),
    ("fm", "FIDE Master"),
    ("cm", "Candidate Master"),
    ("wgm", "Woman Grandmaster"),
    ("wim", "Woman International Master"),
    ("wfm", "Woman FIDE Master"),
    ("wcm", "Woman Candidate Master"),
    ("cgm", "Correspondence GM"),
    ("cim", "Correspondence IM"),
    ("csm", "Correspondence Senior Master"),
    ("clg", "Correspondence Woman Grandmaster"),
    ("clm", "Correspondence Woman Master"),
    ("hgm", "Honorary Grandmaster"),
    ("comp", "Computer"),
];

/// Countries of the `countryTable` of Scid: `(code, English name)`. The code
/// `SCG` appears twice in the original; the first entry is the one found.
/// Two spelling mistakes of the original ("Columbia", "Herzogovina") are
/// corrected.
pub const COUNTRY_TABLE: &[(&str, &str)] = &[
    ("AHO", "Netherlands Antilles"),
    ("AND", "Andorra"),
    ("ARG", "Argentina"),
    ("ARM", "Armenia"),
    ("AUS", "Australia"),
    ("AUT", "Austria"),
    ("AZE", "Azerbaijan"),
    ("BAN", "Bangladesh"),
    ("BAR", "Barbados"),
    ("BEL", "Belgium"),
    ("BER", "Bermuda"),
    ("BIH", "Bosnia & Herzegovina"),
    ("BLR", "Belarus"),
    ("BOL", "Bolivia"),
    ("BRA", "Brazil"),
    ("BUL", "Bulgaria"),
    ("CAN", "Canada"),
    ("CHI", "Chile"),
    ("CHN", "China"),
    ("COL", "Colombia"),
    ("CRC", "Costa Rica"),
    ("CRO", "Croatia"),
    ("CUB", "Cuba"),
    ("CZE", "Czechoslovakia"),
    ("DEN", "Denmark"),
    ("DOM", "Dominican Republic"),
    ("ECU", "Ecuador"),
    ("EGY", "Egypt"),
    ("ENG", "England"),
    ("ESA", "El Salvador"),
    ("ESP", "Spain"),
    ("EST", "Estonia"),
    ("FAI", "Faroe Islands"),
    ("FIN", "Finland"),
    ("FRA", "France"),
    ("GCI", "Guernsey"),
    ("GEO", "Georgia"),
    ("GER", "Germany"),
    ("GRE", "Greece"),
    ("GRL", "Greenland"),
    ("GUA", "Guatemala"),
    ("HUN", "Hungary"),
    ("HKG", "Hong Kong"),
    ("INA", "Indonesia"),
    ("IND", "India"),
    ("INT", "Internet"),
    ("IOM", "Isle of Man"),
    ("IRI", "Iran"),
    ("IRL", "Ireland"),
    ("IRQ", "Iraq"),
    ("ISL", "Iceland"),
    ("ISR", "Israel"),
    ("ITA", "Italy"),
    ("JAM", "Jamaica"),
    ("JCI", "Jersey"),
    ("JPN", "Japan"),
    ("KAZ", "Kazakhstan"),
    ("KGZ", "Kyrgyzstan"),
    ("LAT", "Latvia"),
    ("LIB", "Lebanon"),
    ("LIE", "Liechtenstein"),
    ("LTU", "Lithuania"),
    ("LUX", "Luxembourg"),
    ("MAR", "Morocco"),
    ("MAS", "Malaysia"),
    ("MDA", "Moldova"),
    ("MEX", "Mexico"),
    ("MGL", "Mongolia"),
    ("MKD", "Macedonia"),
    ("MLT", "Malta"),
    ("MNC", "Monaco"),
    ("MYA", "Myanmar"),
    ("NCA", "Nicaragua"),
    ("NCL", "New Caledonia"),
    ("NED", "Netherlands"),
    ("NGR", "Nigeria"),
    ("NOR", "Norway"),
    ("NZL", "New Zealand"),
    ("PAN", "Panama"),
    ("PAR", "Paraguay"),
    ("PER", "Peru"),
    ("PHI", "Philippines"),
    ("PNG", "Papua New Guinea"),
    ("POL", "Poland"),
    ("POR", "Portugal"),
    ("PUR", "Puerto Rico"),
    ("QAT", "Qatar"),
    ("ROM", "Romania"),
    ("RSA", "South Africa"),
    ("RUS", "Russia"),
    ("SCG", "Serbia & Montenegro"),
    ("SCO", "Scotland"),
    ("SIN", "Singapore"),
    ("SLO", "Slovenia"),
    ("SUI", "Switzerland"),
    ("SVK", "Slovakia"),
    ("SWE", "Sweden"),
    ("SYR", "Syria"),
    ("TJK", "Tajikistan"),
    ("TKM", "Turkmenistan"),
    ("TUN", "Tunisia"),
    ("TUR", "Turkey"),
    ("UAE", "United Arab Emirates"),
    ("UKR", "Ukraine"),
    ("URU", "Uruguay"),
    ("USA", "United States of America"),
    ("UZB", "Uzbekistan"),
    ("VEN", "Venezuela"),
    ("VIE", "Vietnam"),
    ("WLS", "Wales"),
    ("YEM", "Yemen"),
    (
        "SCG",
        "Yugoslavia - most are now SCG (Serbia and Montenegro)",
    ),
    ("FIJ", "Fiji"),
    ("YUG", "Yugoslavia"),
];

/// The English name of a title code (any case), if it is in the table.
///
/// ```
/// use mychess_db::spelling::title_name;
///
/// assert_eq!(title_name("GM"), Some("Grandmaster"));
/// assert_eq!(title_name("xx"), None);
/// ```
pub fn title_name(code: &str) -> Option<&'static str> {
    TITLE_TABLE
        .iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(code))
        .map(|&(_, name)| name)
}

/// The English name of a country code (exact, uppercase), if it is in the
/// table. The first of two entries with the same code wins.
///
/// ```
/// use mychess_db::spelling::country_name;
///
/// assert_eq!(country_name("BRA"), Some("Brazil"));
/// assert_eq!(country_name("ZZZ"), None);
/// ```
pub fn country_name(code: &str) -> Option<&'static str> {
    COUNTRY_TABLE
        .iter()
        .find(|(c, _)| *c == code)
        .map(|&(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_have_the_sizes_of_the_original() {
        assert_eq!(TITLE_TABLE.len(), 15);
        assert_eq!(COUNTRY_TABLE.len(), 114);
        assert_eq!(country_name("SCG"), Some("Serbia & Montenegro"));
        assert_eq!(country_name("YUG"), Some("Yugoslavia"));
        assert_eq!(title_name("COMP"), Some("Computer"));
    }
}
