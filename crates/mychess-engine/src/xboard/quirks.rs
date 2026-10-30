//! Engines that do not announce what they can do (spec m4-2, D421).
//!
//! A data table of the 22 engines of the Scid's `tcl/tools/wbdetect.tcl`,
//! recognised by their start-up text. The first entry whose pattern appears
//! in a line of the engine's output wins, as in the Scid. What the engine
//! announces itself through `feature` wins over the table.

/// One engine of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Quirk {
    /// A name for the log.
    pub(crate) name: &'static str,
    /// The text to look for inside a line (`string match *pattern*`).
    pub(crate) pattern: &'static str,
    /// A line that must equal this text exactly (Gromit's `GROMIT`).
    pub(crate) exact: Option<&'static str>,
    /// `analyze` is supported although not announced.
    pub(crate) analyze: Option<bool>,
    /// `setboard` is supported although not announced.
    pub(crate) setboard: Option<bool>,
    /// The thinking time is in whole seconds, not centiseconds.
    pub(crate) whole_seconds: bool,
    /// Commands sent right after the handshake.
    pub(crate) startup: &'static [&'static str],
}

impl Quirk {
    const fn new(name: &'static str, pattern: &'static str) -> Quirk {
        Quirk {
            name,
            pattern,
            exact: None,
            analyze: None,
            setboard: None,
            whole_seconds: false,
            startup: &[],
        }
    }

    const fn analyze(mut self) -> Quirk {
        self.analyze = Some(true);
        self
    }

    const fn setboard(mut self) -> Quirk {
        self.setboard = Some(true);
        self
    }

    const fn seconds(mut self) -> Quirk {
        self.whole_seconds = true;
        self
    }

    fn matches(&self, line: &str) -> bool {
        line.contains(self.pattern) || self.exact.is_some_and(|exact| line.trim() == exact)
    }
}

/// Commands that make Crafty quieter and more responsive (Scid:
/// `wbdetect.tcl`, "log off", "noise 1000", "egtb off", "resign 0").
const CRAFTY_STARTUP: &[&str] = &["log off", "noise 1000", "egtb off", "resign 0"];

/// The table, in the order of `wbdetect.tcl` (first match wins).
pub(crate) const QUIRKS: [Quirk; 22] = [
    Quirk {
        startup: CRAFTY_STARTUP,
        ..Quirk::new("Crafty", "Crafty").analyze().setboard()
    },
    Quirk::new("Amy", "Amy version").analyze(),
    Quirk::new("Baron", "Baron").analyze().setboard().seconds(),
    Quirk::new("Duke", "D U K E").analyze(),
    Quirk::new("ESC", "ESCbook.bin").analyze(),
    Quirk::new("Fortress", "FORTRESS").analyze(),
    Quirk::new("GNU Chess 5", "GNU Chess v5").setboard(),
    Quirk {
        exact: Some("GROMIT"),
        ..Quirk::new("Gromit", "Gromit3").analyze()
    },
    Quirk::new("Jester", "Jester").analyze(),
    Quirk::new("Leila", "Calzerano").analyze(),
    Quirk::new("LordKing", "LordKing").analyze(),
    Quirk::new("Nejmet", "NEJMET").analyze().setboard(),
    Quirk::new("Nejmet 3", "Nejmet").analyze().setboard(),
    Quirk::new("Pharaon", "Pharaon")
        .analyze()
        .setboard()
        .seconds(),
    Quirk::new("Skaki", "Skaki").analyze(),
    Quirk::new("TCB", "EngineControl-TCB").analyze(),
    Quirk::new("ZChess", "ZChess").analyze().seconds(),
    Quirk::new("King of Kings", "King of Kings")
        .analyze()
        .setboard(),
    Quirk::new("EXchess", "EXchess").analyze().setboard(),
    Quirk::new("WildCat", "WildCat version 2.61").analyze(),
    Quirk::new("Phalanx", "Phalanx").analyze().setboard(),
    Quirk::new("Scorpio", "Scorpio").analyze(),
];

/// A table entry that matched, with the facts read from the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Detected {
    pub(crate) quirk: &'static Quirk,
    /// Crafty 18 and later report scores from White's point of view.
    pub(crate) scores_white_pov: bool,
}

/// Looks for the first table entry matching any of `texts` (the engine's
/// `myname` and the lines it printed before `done`, in order).
pub(crate) fn detect<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<Detected> {
    let texts: Vec<&str> = texts.into_iter().collect();
    for text in &texts {
        if let Some(quirk) = QUIRKS.iter().find(|q| q.matches(text)) {
            let scores_white_pov = quirk.name == "Crafty"
                && texts
                    .iter()
                    .find_map(|t| crafty_version(t))
                    .is_some_and(|(major, _)| major >= 18);
            return Some(Detected {
                quirk,
                scores_white_pov,
            });
        }
    }
    None
}

/// Reads `Crafty v23.4` (the banner) or `Crafty-23.4` (`myname`).
pub(crate) fn crafty_version(text: &str) -> Option<(u32, u32)> {
    let at = text.find("Crafty")?;
    let rest = &text[at + "Crafty".len()..];
    let rest = rest.strip_prefix(['-', ' ']).unwrap_or(rest);
    let rest = rest.strip_prefix('v').unwrap_or(rest);
    let major_end = rest.find(|c: char| !c.is_ascii_digit())?;
    let major = rest[..major_end].parse().ok()?;
    let minor_text = rest[major_end..].strip_prefix('.')?;
    let minor_end = minor_text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(minor_text.len());
    let minor = minor_text[..minor_end].parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One representative line per entry (text invented from the patterns).
    const SAMPLES: [(&str, &str); 22] = [
        ("Crafty", "Crafty v23.4 JA (1 cpus)"),
        ("Amy", "Amy version 0.7"),
        ("Baron", "Baron 0.28a"),
        ("Duke", "D U K E 1.1"),
        ("ESC", "loading ESCbook.bin"),
        ("Fortress", "FORTRESS 1.62"),
        ("GNU Chess 5", "GNU Chess v5.02"),
        ("Gromit", "Gromit3.00"),
        ("Jester", "Jester 0.82"),
        ("Leila", "(c) Calzerano"),
        ("LordKing", "LordKing 3.2"),
        ("Nejmet", "NEJMET 2.6.0"),
        ("Nejmet 3", "Nejmet 3.0.1"),
        ("Pharaon", "Pharaon 2.61"),
        ("Skaki", "Skaki 1.19"),
        ("TCB", "EngineControl-TCB 0045"),
        ("ZChess", "ZChess 2.22"),
        ("King of Kings", "King of Kings 2.02"),
        ("EXchess", "EXchess 4.03"),
        ("WildCat", "WildCat version 2.61"),
        ("Phalanx", "Phalanx XXIII"),
        ("Scorpio", "Scorpio 2.7"),
    ];

    #[test]
    fn every_entry_is_recognised_from_its_own_text() {
        for (name, line) in SAMPLES {
            let found = detect([line]).unwrap_or_else(|| panic!("{name} not detected"));
            assert_eq!(found.quirk.name, name, "{line}");
        }
        assert_eq!(QUIRKS.len(), SAMPLES.len());
    }

    #[test]
    fn flags_follow_wbdetect() {
        let get = |name: &str| *QUIRKS.iter().find(|q| q.name == name).unwrap();
        assert!(get("Baron").whole_seconds && get("Pharaon").whole_seconds);
        assert!(get("ZChess").whole_seconds && get("ZChess").setboard.is_none());
        assert_eq!(get("Amy").setboard, None);
        assert_eq!(get("Amy").analyze, Some(true));
        assert_eq!(get("GNU Chess 5").analyze, None);
        assert_eq!(get("GNU Chess 5").setboard, Some(true));
        assert_eq!(get("Crafty").startup, CRAFTY_STARTUP);
        assert!(!get("Crafty").whole_seconds);
    }

    #[test]
    fn gromit_also_matches_the_exact_line() {
        assert_eq!(detect(["GROMIT"]).map(|d| d.quirk.name), Some("Gromit"));
        assert_eq!(detect(["GROMIT 2"]).map(|d| d.quirk.name), None);
        // Stray blanks (CRLF leftovers, indentation) do not hide it.
        assert_eq!(
            detect(["  GROMIT \t"]).map(|d| d.quirk.name),
            Some("Gromit")
        );
    }

    #[test]
    fn crafty_scores_flip_at_version_18() {
        let white = |line: &str| detect([line]).map(|d| d.scores_white_pov);
        assert_eq!(white("Crafty v17.9 JA"), Some(false));
        assert_eq!(white("Crafty v18.0"), Some(true));
        assert_eq!(white("Crafty v23.4 JA (1 cpus)"), Some(true));
        // `myname` form, banner without a version, and the version in a later line.
        assert_eq!(white("Crafty-23.4 JA"), Some(true));
        assert_eq!(white("Crafty"), Some(false));
        assert_eq!(
            detect(["Crafty", "Crafty v19.1"]).map(|d| d.scores_white_pov),
            Some(true)
        );
    }

    #[test]
    fn unknown_text_and_first_match_order() {
        assert_eq!(detect(["Stockfish 17", "readyok"]), None);
        assert_eq!(detect([]), None);
        let both = detect(["Amy version 1", "Crafty v1.0"]).unwrap();
        assert_eq!(both.quirk.name, "Amy");
    }

    #[test]
    fn crafty_version_reader_is_tolerant() {
        assert_eq!(crafty_version("Crafty v23.4 JA"), Some((23, 4)));
        assert_eq!(crafty_version("Crafty-18.0"), Some((18, 0)));
        assert_eq!(crafty_version("Crafty v"), None);
        assert_eq!(crafty_version("Crafty vx.y"), None);
        assert_eq!(crafty_version("nothing"), None);
        assert_eq!(crafty_version("Crafty 99999999999.1"), None);
    }
}
