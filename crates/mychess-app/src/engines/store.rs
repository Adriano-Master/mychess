//! `engines.toml` as text: tolerant reading and rendering (D424).
//!
//! ```toml
//! version = 1
//! next_id = 3
//!
//! [[engine]]
//! id = 1
//! name = "Stockfish"
//! command = "/usr/games/stockfish"
//! args = ["-x"]
//! protocol = "uci"
//! hotkey = "F2"
//! options = [{ name = "Hash", value = "128" }]
//! ```
//!
//! A wrong value only resets its own field, an entry without a name or a
//! command is dropped, and every such remark is reported as a [`LoadIssue`].
//! Only broken TOML or an unknown version fail the whole read.

use std::path::PathBuf;

use mychess_engine::Protocol;
use toml::{Table, Value};

use super::error::{StoreError, ValidationError};
use super::model::{EngineDraft, EngineEntry, EngineId, EngineList, Hotkey, MAX_ENGINES};

/// Version of the file written by this program.
pub const ENGINES_VERSION: u32 = 1;

/// Something the reader fixed or dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoadIssue {
    /// A field had the wrong type or an invalid value and was reset.
    /// `entry` is the 0-based position in the file.
    FieldReset {
        /// Position of the entry in the file.
        entry: usize,
        /// The field.
        field: &'static str,
    },
    /// An entry was dropped.
    EntryDropped {
        /// Position of the entry in the file.
        entry: usize,
        /// Why.
        reason: ValidationError,
    },
    /// An item of `[[engine]]` was not a table and was skipped.
    EntryNotATable {
        /// Position of the entry in the file.
        entry: usize,
    },
    /// A key was already taken by an earlier entry; this one lost it.
    DuplicateHotkey {
        /// Position of the entry in the file.
        entry: usize,
    },
    /// Entries beyond the limit were ignored.
    TooManyEngines {
        /// The limit.
        limit: usize,
    },
}

/// The result of reading `engines.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadedEngineList {
    /// The list.
    pub list: EngineList,
    /// What was fixed on the way.
    pub issues: Vec<LoadIssue>,
}

fn protocol_name(p: Protocol) -> &'static str {
    match p {
        Protocol::XBoard => "xboard",
        _ => "uci",
    }
}

fn path_text(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn entry_table(list: &EngineList, e: &EngineEntry) -> Table {
    let mut t = Table::new();
    let int = |v: u64| Value::Integer(i64::try_from(v).unwrap_or(i64::MAX));
    t.insert("id".into(), int(e.id.0));
    t.insert("name".into(), Value::String(e.name.clone()));
    t.insert("command".into(), Value::String(path_text(&e.command)));
    t.insert(
        "args".into(),
        Value::Array(e.args.iter().cloned().map(Value::String).collect()),
    );
    if let Some(dir) = &e.working_dir {
        t.insert("working_dir".into(), Value::String(path_text(dir)));
    }
    t.insert("protocol".into(), protocol_name(e.protocol).into());
    if let Some(elo) = e.elo {
        t.insert("elo".into(), Value::Integer(i64::from(elo)));
    }
    if let Some(url) = &e.url {
        t.insert("url".into(), Value::String(url.clone()));
    }
    if let Some(when) = e.last_used {
        t.insert("last_used".into(), Value::Integer(when));
    }
    t.insert("multipv".into(), Value::Integer(i64::from(e.multipv)));
    if let Some(low) = e.low_priority {
        t.insert("low_priority".into(), Value::Boolean(low));
    }
    if let Some(key) = list.hotkey_of(e.id) {
        t.insert("hotkey".into(), key.name().into());
    }
    let options = e
        .options
        .iter()
        .map(|(name, value)| {
            let mut o = Table::new();
            o.insert("name".into(), Value::String(name.clone()));
            o.insert("value".into(), Value::String(value.clone()));
            Value::Table(o)
        })
        .collect();
    t.insert("options".into(), Value::Array(options));
    t
}

/// Writes the list as TOML text.
pub fn render_engine_list(list: &EngineList) -> String {
    let mut root = Table::new();
    root.insert("version".into(), Value::Integer(i64::from(ENGINES_VERSION)));
    root.insert(
        "next_id".into(),
        Value::Integer(i64::try_from(list.next_id()).unwrap_or(i64::MAX)),
    );
    let entries = list
        .engines()
        .iter()
        .map(|e| Value::Table(entry_table(list, e)))
        .collect();
    root.insert("engine".into(), Value::Array(entries));
    format!("# MyChess engine list\n{root}")
}

/// Typed access to the fields of one entry, collecting issues.
struct Fields<'a> {
    table: &'a Table,
    entry: usize,
    issues: &'a mut Vec<LoadIssue>,
}

impl Fields<'_> {
    fn get<T>(&mut self, key: &'static str, conv: impl FnOnce(&Value) -> Option<T>) -> Option<T> {
        let value = self.table.get(key)?;
        let out = conv(value);
        if out.is_none() {
            self.issues.push(LoadIssue::FieldReset {
                entry: self.entry,
                field: key,
            });
        }
        out
    }

    fn string(&mut self, key: &'static str) -> Option<String> {
        self.get(key, |v| v.as_str().map(str::to_owned))
    }

    fn int(&mut self, key: &'static str) -> Option<i64> {
        self.get(key, Value::as_integer)
    }
}

fn parse_args(v: &Value) -> Option<Vec<String>> {
    v.as_array()?
        .iter()
        .map(|a| a.as_str().map(str::to_owned))
        .collect()
}

fn parse_options(v: &Value) -> Option<Vec<(String, String)>> {
    v.as_array()?
        .iter()
        .map(|o| {
            let t = o.as_table()?;
            Some((
                t.get("name")?.as_str()?.to_owned(),
                t.get("value")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// 9999-12-31T23:59:59Z in seconds since the Unix epoch.
const LAST_USED_MAX: i64 = 253_402_300_799;

struct Parsed {
    id: Option<u64>,
    key: Option<Hotkey>,
    draft: EngineDraft,
}

fn parse_entry(
    table: &Table,
    entry: usize,
    issues: &mut Vec<LoadIssue>,
) -> Result<Parsed, ValidationError> {
    let mut f = Fields {
        table,
        entry,
        issues,
    };
    let name = f.string("name").unwrap_or_default();
    let command = f.string("command").unwrap_or_default();
    let mut draft = EngineDraft::new(name, command);
    draft.args = f.get("args", parse_args).unwrap_or_default();
    draft.working_dir = f.string("working_dir").map(PathBuf::from);
    draft.protocol = match f.string("protocol").as_deref() {
        Some("xboard") => Protocol::XBoard,
        Some("uci") | None => Protocol::Uci,
        Some(_) => {
            f.issues.push(LoadIssue::FieldReset {
                entry,
                field: "protocol",
            });
            Protocol::Uci
        }
    };
    draft.elo = match table.get("elo") {
        None => None,
        Some(v) => {
            let elo = v
                .as_integer()
                .and_then(|e| u16::try_from(e).ok())
                .filter(|e| *e <= super::model::MAX_ELO);
            if elo.is_none() {
                f.issues.push(LoadIssue::FieldReset {
                    entry,
                    field: "elo",
                });
            }
            elo
        }
    };
    draft.url = f.string("url");
    // A date outside 1970..=9999 cannot be a real start time (and would not
    // format as a date): reset it (D753).
    draft.last_used = f.int("last_used").filter(|t| {
        let ok = (0..=LAST_USED_MAX).contains(t);
        if !ok {
            f.issues.push(LoadIssue::FieldReset {
                entry,
                field: "last_used",
            });
        }
        ok
    });
    draft.options = f.get("options", parse_options).unwrap_or_default();
    draft.multipv = f
        .int("multipv")
        .map_or(1, |m| u8::try_from(m.clamp(1, 255)).unwrap_or(1));
    draft.low_priority = f.get("low_priority", Value::as_bool);
    let id = f.int("id").and_then(|i| u64::try_from(i).ok());
    let key = f.string("hotkey").and_then(|k| {
        let key = Hotkey::from_name(&k);
        if key.is_none() {
            f.issues.push(LoadIssue::FieldReset {
                entry,
                field: "hotkey",
            });
        }
        key
    });
    // Oversized or otherwise invalid fields: drop the entry (normalization).
    let draft = draft.normalized()?;
    Ok(Parsed { id, key, draft })
}

/// Reads the list from TOML text, tolerantly (D424).
///
/// # Errors
/// [`StoreError::Syntax`] when the text is not TOML or `version` is not a
/// positive integer, [`StoreError::NewerVersion`] when the file comes from a
/// newer program. The caller then keeps the list it had.
pub fn load_engine_list(text: &str) -> Result<LoadedEngineList, StoreError> {
    let root: Table = text.parse().map_err(|_| StoreError::Syntax)?;
    match root.get("version") {
        None => {}
        Some(Value::Integer(v)) if *v >= 1 => {
            if *v > i64::from(ENGINES_VERSION) {
                return Err(StoreError::NewerVersion { found: *v });
            }
        }
        Some(_) => return Err(StoreError::Syntax),
    }
    let mut issues = Vec::new();
    let items: &[Value] = match root.get("engine") {
        None => &[],
        Some(Value::Array(a)) => a,
        Some(_) => return Err(StoreError::Syntax),
    };
    let mut parsed = Vec::new();
    for (entry, item) in items.iter().enumerate() {
        if parsed.len() >= MAX_ENGINES {
            issues.push(LoadIssue::TooManyEngines { limit: MAX_ENGINES });
            break;
        }
        let Some(table) = item.as_table() else {
            issues.push(LoadIssue::EntryNotATable { entry });
            continue;
        };
        match parse_entry(table, entry, &mut issues) {
            Ok(p) => parsed.push((entry, p)),
            Err(reason) => issues.push(LoadIssue::EntryDropped { entry, reason }),
        }
    }
    // Ids: keep the first holder of each; the others get fresh ones.
    let file_next = root
        .get("next_id")
        .and_then(Value::as_integer)
        .and_then(|n| u64::try_from(n).ok())
        .unwrap_or(0);
    let mut next = parsed
        .iter()
        .filter_map(|(_, p)| p.id)
        .max()
        .map_or(0, |m| m.saturating_add(1))
        .max(file_next);
    let mut seen = std::collections::HashSet::new();
    let mut engines = Vec::new();
    let mut hotkeys = [None; 3];
    for (entry, p) in parsed {
        let id = match p.id {
            Some(id) if seen.insert(id) => id,
            other => {
                if other.is_some() {
                    issues.push(LoadIssue::FieldReset { entry, field: "id" });
                }
                let id = next;
                next += 1;
                seen.insert(id);
                id
            }
        };
        if let Some(key) = p.key {
            if hotkeys[key.index()].is_some() {
                issues.push(LoadIssue::DuplicateHotkey { entry });
            } else {
                hotkeys[key.index()] = Some(EngineId(id));
            }
        }
        engines.push(EngineEntry::from_draft(EngineId(id), p.draft));
    }
    Ok(LoadedEngineList {
        list: EngineList::from_parts(engines, hotkeys, next),
        issues,
    })
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use proptest::prelude::*;

    fn sample() -> EngineList {
        let mut l = EngineList::new();
        let mut d = EngineDraft::new("Stockfish", "/usr/games/stockfish");
        d.args = vec!["-x".into(), "a b".into()];
        d.working_dir = Some("/tmp/w".into());
        d.elo = Some(3500);
        d.url = Some("https://stockfishchess.org".into());
        d.last_used = Some(1_700_000_000);
        d.options = vec![
            ("Hash".into(), "128".into()),
            ("Skill Level".into(), "3".into()),
        ];
        d.multipv = 3;
        d.low_priority = Some(true);
        let a = l.add(d).unwrap();
        let mut x = EngineDraft::new("Crafty", "crafty");
        x.protocol = Protocol::XBoard;
        let b = l.add(x).unwrap();
        l.set_hotkey(a, Some(Hotkey::F2)).unwrap();
        l.set_hotkey(b, Some(Hotkey::F4)).unwrap();
        l
    }

    #[test]
    fn round_trip_sample() {
        let list = sample();
        let loaded = load_engine_list(&render_engine_list(&list)).unwrap();
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        assert_eq!(loaded.list, list);
    }

    #[test]
    fn empty_text_and_missing_sections_are_an_empty_list() {
        assert!(load_engine_list("").unwrap().list.is_empty());
        assert!(load_engine_list("version = 1\n").unwrap().list.is_empty());
    }

    #[test]
    fn broken_toml_and_bad_header_are_typed_errors() {
        assert_eq!(load_engine_list("[[engine"), Err(StoreError::Syntax));
        assert_eq!(load_engine_list("version = \"x\""), Err(StoreError::Syntax));
        assert_eq!(load_engine_list("version = 0"), Err(StoreError::Syntax));
        assert_eq!(load_engine_list("engine = 3"), Err(StoreError::Syntax));
        assert_eq!(
            load_engine_list("version = 9\n"),
            Err(StoreError::NewerVersion { found: 9 })
        );
    }

    #[test]
    fn bad_field_resets_only_that_field() {
        let text = r#"
[[engine]]
name = "A"
command = "a"
elo = "high"
args = [1, 2]
protocol = "gopher"
multipv = 9999
unknown = true
"#;
        let loaded = load_engine_list(text).unwrap();
        let e = &loaded.list.engines()[0];
        assert_eq!(
            (e.elo, e.args.clone(), e.protocol, e.multipv),
            (None, vec![], Protocol::Uci, 255)
        );
        let fields: Vec<_> = loaded
            .issues
            .iter()
            .filter_map(|i| match i {
                LoadIssue::FieldReset { field, .. } => Some(*field),
                _ => None,
            })
            .collect();
        assert_eq!(fields, ["args", "protocol", "elo"]);
    }

    #[test]
    fn elo_out_of_range_is_reset() {
        let text = "[[engine]]\nname=\"A\"\ncommand=\"a\"\nelo = 5000\n";
        let loaded = load_engine_list(text).unwrap();
        assert_eq!(loaded.list.engines()[0].elo, None);
        assert!(loaded.issues.contains(&LoadIssue::FieldReset {
            entry: 0,
            field: "elo"
        }));
        let text = "[[engine]]\nname=\"A\"\ncommand=\"a\"\nelo = -3\n";
        assert_eq!(load_engine_list(text).unwrap().list.engines()[0].elo, None);
    }

    #[test]
    fn last_used_outside_1970_to_9999_is_reset() {
        for bad in ["-1", "253402300800"] {
            let text = format!("[[engine]]\nname=\"A\"\ncommand=\"a\"\nlast_used = {bad}\n");
            let loaded = load_engine_list(&text).unwrap();
            assert_eq!(loaded.list.engines()[0].last_used, None, "{bad}");
            assert!(loaded.issues.contains(&LoadIssue::FieldReset {
                entry: 0,
                field: "last_used"
            }));
        }
        let text = "[[engine]]\nname=\"A\"\ncommand=\"a\"\nlast_used = 253402300799\n";
        let loaded = load_engine_list(text).unwrap();
        assert_eq!(loaded.list.engines()[0].last_used, Some(253_402_300_799));
        assert!(loaded.issues.is_empty());
    }

    #[test]
    fn entries_without_name_or_command_are_dropped() {
        let text = "[[engine]]\ncommand=\"a\"\n[[engine]]\nname=\"B\"\n[[engine]]\nname=\"C\"\ncommand=\"c\"\n";
        let loaded = load_engine_list(text).unwrap();
        assert_eq!(loaded.list.len(), 1);
        assert_eq!(loaded.list.engines()[0].name, "C");
        assert_eq!(loaded.issues.len(), 2);
        assert!(matches!(
            loaded.issues[0],
            LoadIssue::EntryDropped {
                entry: 0,
                reason: ValidationError::EmptyName
            }
        ));
    }

    #[test]
    fn duplicate_hotkey_first_wins_and_duplicate_ids_are_renumbered() {
        let text = r#"
[[engine]]
id = 5
name = "A"
command = "a"
hotkey = "F2"
[[engine]]
id = 5
name = "B"
command = "b"
hotkey = "F2"
"#;
        let loaded = load_engine_list(text).unwrap();
        let l = &loaded.list;
        let (a, b) = (l.engines()[0].id, l.engines()[1].id);
        assert_ne!(a, b);
        assert_eq!(l.hotkey_owner(Hotkey::F2), Some(a));
        assert_eq!(l.hotkey_of(b), None);
        assert!(
            loaded
                .issues
                .contains(&LoadIssue::DuplicateHotkey { entry: 1 })
        );
        assert!(l.next_id() > a.0.max(b.0));
    }

    #[test]
    fn too_many_engines_are_ignored() {
        let mut text = String::new();
        for i in 0..MAX_ENGINES + 5 {
            write!(text, "[[engine]]\nname=\"e{i}\"\ncommand=\"c\"\n").unwrap();
        }
        let loaded = load_engine_list(&text).unwrap();
        assert_eq!(loaded.list.len(), MAX_ENGINES);
        assert!(
            loaded
                .issues
                .contains(&LoadIssue::TooManyEngines { limit: MAX_ENGINES })
        );
    }

    #[test]
    fn next_id_survives_a_round_trip_after_removal() {
        let mut l = sample();
        let last = l.engines()[1].id;
        l.remove(last).unwrap();
        let again = load_engine_list(&render_engine_list(&l)).unwrap().list;
        assert_eq!(again.next_id(), l.next_id());
    }

    fn arb_draft() -> impl Strategy<Value = EngineDraft> {
        (
            "[^ \\s]{1,10}",
            "\\PC{1,12}",
            proptest::collection::vec("\\PC{0,8}", 0..4),
            proptest::option::of("\\PC{1,8}"),
            any::<bool>(),
            proptest::option::of(1u16..=4000),
            proptest::collection::vec(("\\PC{0,6}", "\\PC{0,6}"), 0..3),
            (
                1u8..=255,
                proptest::option::of(any::<bool>()),
                proptest::option::of(0..=LAST_USED_MAX),
            ),
        )
            .prop_map(
                |(name, cmd, args, dir, xb, elo, options, (multipv, low, last))| {
                    let mut d = EngineDraft::new(name, cmd.trim_start().to_owned() + "x");
                    d.args = args;
                    d.working_dir = dir.map(PathBuf::from);
                    d.protocol = if xb { Protocol::XBoard } else { Protocol::Uci };
                    d.elo = elo;
                    d.options = options;
                    d.multipv = multipv;
                    d.low_priority = low;
                    d.last_used = last;
                    d
                },
            )
    }

    proptest! {
        #[test]
        fn list_round_trips(
            drafts in proptest::collection::vec(arb_draft(), 0..6),
            keys in proptest::collection::vec((0usize..6, 0usize..4), 0..6),
            removals in proptest::collection::vec(0usize..6, 0..3),
        ) {
            let mut list = EngineList::new();
            for d in drafts {
                list.add(d).unwrap();
            }
            for (i, k) in keys {
                if let Some(e) = list.engines().get(i) {
                    let id = e.id;
                    list.set_hotkey(id, Hotkey::ALL.get(k).copied()).unwrap();
                }
            }
            for i in removals {
                if let Some(e) = list.engines().get(i) {
                    let id = e.id;
                    list.remove(id).unwrap();
                }
            }
            let loaded = load_engine_list(&render_engine_list(&list)).unwrap();
            prop_assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
            prop_assert_eq!(loaded.list, list);
        }

        #[test]
        fn load_never_panics(text in "\\PC{0,200}") {
            drop(load_engine_list(&text));
        }
    }
}
