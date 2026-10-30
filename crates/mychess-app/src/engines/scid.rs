//! Import of the engine list of the Scid vs PC (`engines.dat`, D432).
//!
//! The file is a Tcl script made of `engine { Name ... Cmd ... }` commands
//! (`tcl/tools/analysis.tcl` 162-226 of the Scid). It is **never evaluated**:
//! [`parse_scid_engines_dat`] is a small reader of Tcl words (braces, double
//! quotes, backslashes, `#` comments) that recognizes only the `engine`
//! command. `$variables`, `[commands]` and every other command are plain text
//! or ignored. Bounded in input size, nesting depth, number of words and
//! number of engines; it never panics and never recurses.
//!
//! Known gaps: `\xNN` and `\uNNNN` escapes are read as the letter after the
//! backslash.

use std::path::{Path, PathBuf};

use mychess_engine::Protocol;

use super::args::split_args;
use super::model::{EngineDraft, EngineId, EngineList, MAX_ELO, MAX_ITEMS};

/// Largest input read, in bytes; the rest is ignored.
pub const MAX_SCID_BYTES: usize = 4 << 20;
/// Most engines taken from one file.
pub const MAX_SCID_ENGINES: usize = 1024;
/// Deepest nesting of braces accepted.
pub const MAX_BRACE_DEPTH: usize = 32;
/// Most words in one Tcl list.
const MAX_WORDS: usize = 16_384;
/// Most words in one top-level command.
const MAX_COMMAND_WORDS: usize = 16;

/// One `engine {...}` block, with the fields of the Scid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScidEngine {
    /// `Name`.
    pub name: String,
    /// `Cmd`.
    pub command: String,
    /// `Args`, the arguments as one line of text.
    pub args_text: String,
    /// `Dir`.
    pub dir: String,
    /// `Elo`, when it is a rating in `1..=MAX_ELO`.
    pub elo: Option<u16>,
    /// `URL`.
    pub url: String,
    /// `UCI`: whether the engine speaks UCI (otherwise XBoard).
    pub uci: bool,
    /// `UCIoptions`: `{name value}` pairs.
    pub options: Vec<(String, String)>,
}

/// What [`parse_scid_engines_dat`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScidImport {
    /// The blocks that had `Name`, `Cmd` and `Dir`.
    pub engines: Vec<ScidEngine>,
    /// Blocks skipped: missing or empty `Name`/`Cmd`/`Dir`, odd key/value
    /// count, unreadable.
    pub skipped: usize,
    /// The input or the number of engines hit a limit, or a word was
    /// malformed (unbalanced braces, nesting too deep): the rest was ignored.
    pub incomplete: bool,
}

/// A remark about an imported engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScidWarning {
    /// `Cmd` or `Dir` is relative to the directory of the Scid.
    RelativePath,
    /// `Args` has an unclosed quote; it was split on whitespace instead.
    ArgsUnparsable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordError {
    Unterminated,
    TooDeep,
    TooMany,
}

struct Reader {
    chars: Vec<char>,
    pos: usize,
    /// Inside a list newlines and `;` are ordinary; at the top level they end
    /// a command.
    list: bool,
}

impl Reader {
    fn new(text: &str, list: bool) -> Reader {
        Reader {
            chars: text.chars().collect(),
            pos: 0,
            list,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    /// Reads the character after a backslash and returns its replacement.
    fn escape(&mut self) -> char {
        match self.bump() {
            None => '\\',
            Some('n') => '\n',
            Some('t') => '\t',
            Some('r') => '\r',
            Some('\n') => {
                while matches!(self.peek(), Some(' ' | '\t')) {
                    self.pos += 1;
                }
                ' '
            }
            Some(c) => c,
        }
    }

    /// Skips blanks; newlines too when `newlines`; a backslash-newline always.
    fn skip_blanks(&mut self, newlines: bool) {
        while let Some(c) = self.peek() {
            let continuation = c == '\\' && self.chars.get(self.pos + 1) == Some(&'\n');
            if c == ' ' || c == '\t' || c == '\r' || (newlines && c == '\n') {
                self.pos += 1;
            } else if continuation {
                self.pos += 2;
            } else {
                break;
            }
        }
    }

    fn read_word(&mut self) -> Result<String, WordError> {
        match self.peek() {
            Some('{') => self.read_braced(),
            Some('"') => self.read_quoted(),
            _ => Ok(self.read_bare()),
        }
    }

    fn read_braced(&mut self) -> Result<String, WordError> {
        self.pos += 1;
        let mut depth = 1usize;
        let mut out = String::new();
        loop {
            match self.bump().ok_or(WordError::Unterminated)? {
                '\\' => {
                    out.push('\\');
                    if let Some(c) = self.bump() {
                        out.push(c);
                    }
                }
                '{' => {
                    depth += 1;
                    if depth > MAX_BRACE_DEPTH {
                        return Err(WordError::TooDeep);
                    }
                    out.push('{');
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push('}');
                }
                c => out.push(c),
            }
        }
    }

    fn read_quoted(&mut self) -> Result<String, WordError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.bump().ok_or(WordError::Unterminated)? {
                '"' => return Ok(out),
                '\\' => out.push(self.escape()),
                c => out.push(c),
            }
        }
    }

    fn read_bare(&mut self) -> String {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c.is_whitespace() || (!self.list && c == ';') {
                break;
            }
            self.pos += 1;
            if c == '\\' {
                out.push(self.escape());
            } else {
                out.push(c);
            }
        }
        out
    }
}

/// Splits `text` as a Tcl list.
fn parse_list(text: &str) -> Result<Vec<String>, WordError> {
    let mut reader = Reader::new(text, true);
    let mut words = Vec::new();
    loop {
        reader.skip_blanks(true);
        if reader.peek().is_none() {
            return Ok(words);
        }
        if words.len() >= MAX_WORDS {
            return Err(WordError::TooMany);
        }
        words.push(reader.read_word()?);
    }
}

fn parse_options(text: &str) -> Vec<(String, String)> {
    let Ok(pairs) = parse_list(text) else {
        return Vec::new();
    };
    pairs
        .iter()
        .filter_map(|pair| {
            let mut parts = parse_list(pair).ok()?.into_iter();
            let name = parts.next().filter(|n| !n.is_empty())?;
            Some((name, parts.next().unwrap_or_default()))
        })
        .take(MAX_ITEMS)
        .collect()
}

fn parse_tcl_bool(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    ) || text.trim().parse::<f64>().is_ok_and(|n| n != 0.0)
}

fn parse_block(body: &str) -> Option<ScidEngine> {
    let words = parse_list(body).ok()?;
    if words.len() % 2 != 0 {
        return None;
    }
    let mut fields = std::collections::HashMap::new();
    for pair in words.chunks(2) {
        if let [key, value] = pair {
            fields.insert(key.as_str(), value.as_str());
        }
    }
    let get = |k: &str| fields.get(k).copied();
    let name = get("Name")?.trim().to_owned();
    let command = get("Cmd")?.trim().to_owned();
    let dir = get("Dir")?.trim().to_owned();
    if name.is_empty() || command.is_empty() {
        return None;
    }
    Some(ScidEngine {
        name,
        command,
        args_text: get("Args").unwrap_or_default().to_owned(),
        dir,
        elo: get("Elo")
            .and_then(|e| e.trim().parse::<u16>().ok())
            .filter(|e| (1..=MAX_ELO).contains(e)),
        url: get("URL").unwrap_or_default().trim().to_owned(),
        uci: get("UCI").is_some_and(parse_tcl_bool),
        options: get("UCIoptions").map(parse_options).unwrap_or_default(),
    })
}

/// Reads the `engine {...}` blocks of an `engines.dat`. Never evaluates
/// anything and never fails: what cannot be read is counted in
/// [`ScidImport::skipped`] or flagged by [`ScidImport::incomplete`].
pub fn parse_scid_engines_dat(text: &str) -> ScidImport {
    let mut import = ScidImport::default();
    let mut text = text;
    if text.len() > MAX_SCID_BYTES {
        let mut cut = MAX_SCID_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text = &text[..cut];
        import.incomplete = true;
    }
    let mut reader = Reader::new(text, false);
    'commands: loop {
        reader.skip_blanks(true);
        while reader.peek() == Some(';') {
            reader.pos += 1;
            reader.skip_blanks(true);
        }
        match reader.peek() {
            None => break,
            Some('#') => {
                while let Some(c) = reader.bump() {
                    if c == '\\' {
                        reader.bump();
                    } else if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            Some(_) => {}
        }
        let mut words = Vec::new();
        loop {
            reader.skip_blanks(false);
            if matches!(reader.peek(), None | Some('\n' | ';')) {
                break;
            }
            if words.len() >= MAX_COMMAND_WORDS {
                import.incomplete = true;
                break 'commands;
            }
            if let Ok(w) = reader.read_word() {
                words.push(w);
            } else {
                import.incomplete = true;
                break 'commands;
            }
        }
        if words.first().map(String::as_str) != Some("engine") {
            continue;
        }
        if import.engines.len() >= MAX_SCID_ENGINES {
            import.incomplete = true;
            break;
        }
        match words.as_slice() {
            [_, body] => match parse_block(body) {
                Some(engine) => import.engines.push(engine),
                None => import.skipped += 1,
            },
            _ => import.skipped += 1,
        }
    }
    import
}

fn is_relative_with_directories(path: &str) -> bool {
    let p = Path::new(path);
    !p.is_absolute() && p.components().count() > 1
}

impl ScidEngine {
    /// The draft of this engine and the remarks about it. `Cmd` and `Dir`
    /// stay as written (D432).
    pub fn to_draft(&self) -> (EngineDraft, Vec<ScidWarning>) {
        let mut warnings = Vec::new();
        let mut draft = EngineDraft::new(self.name.clone(), PathBuf::from(&self.command));
        draft.args = split_args(&self.args_text).unwrap_or_else(|_| {
            warnings.push(ScidWarning::ArgsUnparsable);
            self.args_text
                .split_whitespace()
                .map(str::to_owned)
                .collect()
        });
        if !self.dir.is_empty() {
            draft.working_dir = Some(PathBuf::from(&self.dir));
        }
        if is_relative_with_directories(&self.command) || self.dir.starts_with('.') {
            warnings.push(ScidWarning::RelativePath);
        }
        draft.protocol = if self.uci {
            Protocol::Uci
        } else {
            Protocol::XBoard
        };
        draft.elo = self.elo;
        draft.url = Some(self.url.clone()).filter(|u| !u.is_empty());
        draft.options.clone_from(&self.options);
        (draft, warnings)
    }
}

/// A remark about one imported engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportWarning {
    /// The engine's name.
    pub engine: String,
    /// The remark.
    pub kind: ScidWarning,
}

/// The outcome of [`EngineList::import_scid`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    /// The engines added, in order.
    pub added: Vec<EngineId>,
    /// Skipped because an engine with the same name and command exists.
    pub duplicates: usize,
    /// Skipped because the block was unusable or the entry invalid.
    pub invalid: usize,
    /// Skipped because the list is full.
    pub over_limit: usize,
    /// The input hit a limit or was malformed (see [`ScidImport::incomplete`]).
    pub incomplete: bool,
    /// Remarks about the engines added.
    pub warnings: Vec<ImportWarning>,
}

impl EngineList {
    /// Adds the engines of an `engines.dat` that are not there yet (same name
    /// and command), without keys. Never fails: the report says what happened.
    pub fn import_scid(&mut self, import: &ScidImport) -> ImportReport {
        let mut report = ImportReport {
            invalid: import.skipped,
            incomplete: import.incomplete,
            ..ImportReport::default()
        };
        for engine in &import.engines {
            let (draft, warnings) = engine.to_draft();
            let duplicate = self
                .engines()
                .iter()
                .any(|e| e.name == draft.name.trim() && e.command == draft.command);
            if duplicate {
                report.duplicates += 1;
                continue;
            }
            match self.add(draft) {
                Ok(id) => {
                    report.added.push(id);
                    report
                        .warnings
                        .extend(warnings.into_iter().map(|kind| ImportWarning {
                            engine: engine.name.clone(),
                            kind,
                        }));
                }
                Err(super::error::ListError::TooManyEngines { .. }) => report.over_limit += 1,
                Err(_) => report.invalid += 1,
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use proptest::prelude::*;

    const SAMPLE: &str = r#"# Analysis Engines configuration for Scid vs. PC 4.27

engine {
  Name Stockfish
  Cmd  /usr/games/stockfish
  Args {-t 4 "my file"}
  Dir  /home/me
  Elo  3500
  Time 1700000000
  URL  https://stockfishchess.org
  UCI  1
  UCIoptions {{Hash 128} {{Skill Level} 3} {BookFile {/a b/book.bin}}}
}

engine {
  Name "Phalanx XXV"
  Cmd  phalanx
  Dir  .
  Elo  0
  UCI  0
}
engine "Name Toga
        Cmd fruit
        Dir /x"
engine { Name NoCmd Dir . }
set foo [exec rm -rf /]
engine { Name Odd Cmd x Dir . Extra }
"#;

    #[test]
    fn reads_the_scid_format() {
        let import = parse_scid_engines_dat(SAMPLE);
        assert!(!import.incomplete);
        assert_eq!(import.skipped, 2);
        let names: Vec<_> = import.engines.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Stockfish", "Phalanx XXV", "Toga"]);
        let sf = &import.engines[0];
        assert_eq!(sf.command, "/usr/games/stockfish");
        assert_eq!(sf.args_text, "-t 4 \"my file\"");
        assert_eq!((sf.elo, sf.uci), (Some(3500), true));
        assert_eq!(sf.url, "https://stockfishchess.org");
        assert_eq!(
            sf.options,
            [
                ("Hash".to_owned(), "128".to_owned()),
                ("Skill Level".to_owned(), "3".to_owned()),
                ("BookFile".to_owned(), "/a b/book.bin".to_owned()),
            ]
        );
        let ph = &import.engines[1];
        assert_eq!((ph.elo, ph.uci, ph.dir.as_str()), (None, false, "."));
        assert_eq!(import.engines[2].command, "fruit");
    }

    #[test]
    fn nothing_is_evaluated() {
        let text = "engine {Name {$x [exec echo hi]} Cmd {[exec ls]} Dir {$HOME}}\n";
        let import = parse_scid_engines_dat(text);
        assert_eq!(import.engines[0].name, "$x [exec echo hi]");
        assert_eq!(import.engines[0].command, "[exec ls]");
        assert_eq!(import.engines[0].dir, "$HOME");
    }

    #[test]
    fn mapping_to_a_draft_and_warnings() {
        let import = parse_scid_engines_dat(SAMPLE);
        let (d, w) = import.engines[0].to_draft();
        assert!(w.is_empty());
        assert_eq!(d.args, ["-t", "4", "my file"]);
        assert_eq!(d.protocol, Protocol::Uci);
        assert_eq!(d.working_dir, Some(PathBuf::from("/home/me")));
        let (d, w) = import.engines[1].to_draft();
        assert_eq!(d.protocol, Protocol::XBoard);
        assert_eq!(w, [ScidWarning::RelativePath]);
        let bad = ScidEngine {
            args_text: "a 'b".into(),
            ..import.engines[0].clone()
        };
        let (d, w) = bad.to_draft();
        assert_eq!(d.args, ["a", "'b"]);
        assert_eq!(w, [ScidWarning::ArgsUnparsable]);
    }

    #[test]
    fn import_skips_duplicates_and_reports() {
        let import = parse_scid_engines_dat(SAMPLE);
        let mut list = EngineList::new();
        list.add(EngineDraft::new("Stockfish", "/usr/games/stockfish"))
            .unwrap();
        let report = list.import_scid(&import);
        assert_eq!(report.added.len(), 2);
        assert_eq!((report.duplicates, report.invalid), (1, 2));
        assert_eq!(list.len(), 3);
        assert_eq!(report.warnings.len(), 1);
        let again = list.import_scid(&import);
        assert!(again.added.is_empty());
        assert_eq!(again.duplicates, 3);
    }

    #[test]
    fn empty_and_commented_files() {
        assert_eq!(parse_scid_engines_dat(""), ScidImport::default());
        let import = parse_scid_engines_dat("# only a comment \\\n continued\n");
        assert!(import.engines.is_empty() && !import.incomplete);
    }

    #[test]
    fn deep_nesting_is_rejected_without_overflow() {
        let deep = format!("engine {}x{}\n", "{".repeat(100_000), "}".repeat(100_000));
        let import = parse_scid_engines_dat(&deep);
        assert!(import.engines.is_empty() && import.incomplete);
        let inner = format!(
            "{}x{}",
            "{".repeat(MAX_BRACE_DEPTH - 2),
            "}".repeat(MAX_BRACE_DEPTH - 2)
        );
        let at_limit = format!("engine {{Name {{{inner}}} Cmd c Dir .}}");
        assert_eq!(parse_scid_engines_dat(&at_limit).engines.len(), 1);
    }

    #[test]
    fn unbalanced_input_is_flagged() {
        let import = parse_scid_engines_dat("engine { Name a Cmd b Dir .");
        assert!(import.engines.is_empty() && import.incomplete);
        let import = parse_scid_engines_dat("engine \"Name a");
        assert!(import.incomplete);
    }

    #[test]
    fn engine_count_and_size_are_capped() {
        let mut text = String::new();
        for i in 0..MAX_SCID_ENGINES + 10 {
            writeln!(text, "engine {{Name e{i} Cmd c Dir .}}").unwrap();
        }
        let import = parse_scid_engines_dat(&text);
        assert_eq!(import.engines.len(), MAX_SCID_ENGINES);
        assert!(import.incomplete);
        let huge = "é".repeat(MAX_SCID_BYTES);
        assert!(parse_scid_engines_dat(&huge).incomplete);
    }

    #[test]
    fn many_commands_in_one_line_are_capped() {
        let line = format!("x {}\n", "a ".repeat(100));
        assert!(parse_scid_engines_dat(&line).incomplete);
    }

    #[test]
    fn escapes_in_quotes_and_bare_words() {
        let text = "engine \"Name {a\\\"b} Cmd {c d} Dir x\"\nengine {Name n Cmd c\\ d Dir x}\n";
        let import = parse_scid_engines_dat(text);
        assert_eq!(import.engines[0].name, "a\"b");
        assert_eq!(import.engines[0].command, "c d");
        assert_eq!(import.engines[1].command, "c d");
    }

    proptest! {
        #[test]
        fn garbage_never_panics(text in "\\PC{0,300}") {
            drop(parse_scid_engines_dat(&text));
        }

        #[test]
        fn brace_soup_never_panics(text in "[{}\"\\\\ \\n;#a-zA-Z$\\[\\]]{0,300}") {
            let import = parse_scid_engines_dat(&text);
            let mut list = EngineList::new();
            drop(list.import_scid(&import));
        }

        #[test]
        fn written_blocks_read_back(
            name in "[a-zA-Z0-9 ]{1,12}",
            cmd in "[a-zA-Z0-9/._-]{1,12}",
            elo in 0u16..=4000,
        ) {
            let text = format!(
                "engine {{\n  Name {{{name}}}\n  Cmd {{{cmd}}}\n  Dir {{.}}\n  Elo {elo}\n}}\n"
            );
            let import = parse_scid_engines_dat(&text);
            prop_assume!(!name.trim().is_empty());
            prop_assert_eq!(import.engines.len(), 1);
            prop_assert_eq!(import.engines[0].name.as_str(), name.trim());
            prop_assert_eq!(import.engines[0].command.as_str(), cmd.as_str());
        }
    }
}
