//! The engine list: entries, hotkeys and the pure operations on them
//! (spec M4-3, D425-D429). Nothing here touches the disk.

use std::path::PathBuf;

use mychess_engine::Protocol;

use super::error::{ListError, ValidationError};

/// Largest Elo rating accepted (the limit of the Scid, `sc_info limit elo`).
pub const MAX_ELO: u16 = 4000;
/// Most engines a list holds.
pub const MAX_ENGINES: usize = 256;
/// Most arguments (or saved options) one engine holds.
pub const MAX_ITEMS: usize = 512;
/// Longest text field, in characters.
pub const MAX_TEXT_CHARS: usize = 4096;
/// Largest `MultiPV` remembered.
pub const MAX_MULTIPV: u8 = 255;
/// The suffix `copy` adds to the name (Scid: `analysis.tcl:552-556`).
pub const COPY_SUFFIX: &str = "-new";

/// A stable engine identity (D425); never reused inside one list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EngineId(pub u64);

/// A key that starts an engine (F2, F3 or F4, as in the Scid).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Hotkey {
    /// F2.
    F2,
    /// F3.
    F3,
    /// F4.
    F4,
}

impl Hotkey {
    /// All keys, in order.
    pub const ALL: [Hotkey; 3] = [Hotkey::F2, Hotkey::F3, Hotkey::F4];

    /// Position in [`Hotkey::ALL`].
    pub fn index(self) -> usize {
        match self {
            Hotkey::F2 => 0,
            Hotkey::F3 => 1,
            Hotkey::F4 => 2,
        }
    }

    /// The name used in the file and on screen.
    pub fn name(self) -> &'static str {
        match self {
            Hotkey::F2 => "F2",
            Hotkey::F3 => "F3",
            Hotkey::F4 => "F4",
        }
    }

    /// The key called `name` (ignoring case).
    pub fn from_name(name: &str) -> Option<Hotkey> {
        Hotkey::ALL
            .into_iter()
            .find(|k| k.name().eq_ignore_ascii_case(name))
    }
}

/// The editable fields of an engine (everything but the id and the key).
/// Input of [`EngineList::add`] and [`EngineList::edit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineDraft {
    /// Display name (required).
    pub name: String,
    /// The executable (required).
    pub command: PathBuf,
    /// Arguments, one per element.
    pub args: Vec<String>,
    /// Working directory; the executable's own when `None`.
    pub working_dir: Option<PathBuf>,
    /// The protocol.
    pub protocol: Protocol,
    /// Elo rating, `1..=MAX_ELO`; `None` = unknown.
    pub elo: Option<u16>,
    /// Home page.
    pub url: Option<String>,
    /// Last start, in seconds since the Unix epoch.
    pub last_used: Option<i64>,
    /// Saved option values, in order (UCI), as text.
    pub options: Vec<(String, String)>,
    /// The last `MultiPV` chosen, `1..=MAX_MULTIPV`.
    pub multipv: u8,
    /// Low CPU priority; `None` follows the global preference (D431).
    pub low_priority: Option<bool>,
}

impl EngineDraft {
    /// A draft with the defaults: UCI, `MultiPV` 1, no extras.
    pub fn new(name: impl Into<String>, command: impl Into<PathBuf>) -> EngineDraft {
        EngineDraft {
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
            working_dir: None,
            protocol: Protocol::Uci,
            elo: None,
            url: None,
            last_used: None,
            options: Vec::new(),
            multipv: 1,
            low_priority: None,
        }
    }

    /// Checks the draft and returns its normalized form: name and URL
    /// trimmed (an empty URL is dropped), `Elo` 0 becomes unknown, `MultiPV`
    /// clamped. Pure: it does not look at the disk (see
    /// [`check_command`](super::check_command) for that).
    pub fn normalized(mut self) -> Result<EngineDraft, ValidationError> {
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() {
            return Err(ValidationError::EmptyName);
        }
        if self.command.to_str().is_none_or(|c| c.trim().is_empty()) {
            return Err(if self.command.to_str().is_none() {
                ValidationError::NonUtf8Path
            } else {
                ValidationError::EmptyCommand
            });
        }
        if self
            .working_dir
            .as_ref()
            .is_some_and(|d| d.to_str().is_none())
        {
            return Err(ValidationError::NonUtf8Path);
        }
        if self
            .working_dir
            .as_ref()
            .is_some_and(|d| d.as_os_str().is_empty())
        {
            self.working_dir = None;
        }
        self.elo = match self.elo {
            Some(0) | None => None,
            Some(e) if e > MAX_ELO => return Err(ValidationError::EloOutOfRange(e)),
            elo => elo,
        };
        self.url = self
            .url
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty());
        self.multipv = self.multipv.clamp(1, MAX_MULTIPV);
        check_text("name", &self.name)?;
        check_text("command", &self.command.to_string_lossy())?;
        if let Some(url) = &self.url {
            check_text("url", url)?;
        }
        if self.args.len() > MAX_ITEMS {
            return Err(ValidationError::TooManyItems {
                field: "args",
                limit: MAX_ITEMS,
            });
        }
        if self.options.len() > MAX_ITEMS {
            return Err(ValidationError::TooManyItems {
                field: "options",
                limit: MAX_ITEMS,
            });
        }
        for arg in &self.args {
            check_text("args", arg)?;
        }
        for (name, value) in &self.options {
            check_text("options", name)?;
            check_text("options", value)?;
        }
        Ok(self)
    }
}

fn check_text(field: &'static str, text: &str) -> Result<(), ValidationError> {
    if text.chars().take(MAX_TEXT_CHARS + 1).count() > MAX_TEXT_CHARS {
        Err(ValidationError::TextTooLong {
            field,
            limit: MAX_TEXT_CHARS,
        })
    } else {
        Ok(())
    }
}

/// One engine of the list (D426).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineEntry {
    /// The stable identity.
    pub id: EngineId,
    /// Display name.
    pub name: String,
    /// The executable.
    pub command: PathBuf,
    /// Arguments, one per element.
    pub args: Vec<String>,
    /// Working directory; the executable's own when `None`.
    pub working_dir: Option<PathBuf>,
    /// The protocol.
    pub protocol: Protocol,
    /// Elo rating; `None` = unknown.
    pub elo: Option<u16>,
    /// Home page.
    pub url: Option<String>,
    /// Last start, in seconds since the Unix epoch.
    pub last_used: Option<i64>,
    /// Saved option values, in order (UCI), as text.
    pub options: Vec<(String, String)>,
    /// The last `MultiPV` chosen.
    pub multipv: u8,
    /// Low CPU priority; `None` follows the global preference.
    pub low_priority: Option<bool>,
}

impl EngineEntry {
    pub(super) fn from_draft(id: EngineId, d: EngineDraft) -> EngineEntry {
        EngineEntry {
            id,
            name: d.name,
            command: d.command,
            args: d.args,
            working_dir: d.working_dir,
            protocol: d.protocol,
            elo: d.elo,
            url: d.url,
            last_used: d.last_used,
            options: d.options,
            multipv: d.multipv,
            low_priority: d.low_priority,
        }
    }

    /// The editable fields, for the edit dialog.
    pub fn to_draft(&self) -> EngineDraft {
        EngineDraft {
            name: self.name.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            working_dir: self.working_dir.clone(),
            protocol: self.protocol,
            elo: self.elo,
            url: self.url.clone(),
            last_used: self.last_used,
            options: self.options.clone(),
            multipv: self.multipv,
            low_priority: self.low_priority,
        }
    }
}

/// The ordered list of engines and their keys.
///
/// Invariants: ids are unique, a key belongs to at most one engine and an
/// engine has at most one key, `next_id` is above every id in use.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineList {
    engines: Vec<EngineEntry>,
    hotkeys: [Option<EngineId>; 3],
    next_id: u64,
}

impl EngineList {
    /// An empty list.
    pub fn new() -> EngineList {
        EngineList::default()
    }

    /// Builds a list from parts that the loader already made consistent.
    pub(super) fn from_parts(
        engines: Vec<EngineEntry>,
        hotkeys: [Option<EngineId>; 3],
        next_id: u64,
    ) -> EngineList {
        let floor = engines.iter().map(|e| e.id.0).max().map_or(0, |m| m + 1);
        EngineList {
            engines,
            hotkeys,
            next_id: next_id.max(floor),
        }
    }

    /// The engines, in list order (which defines the main engine, D434).
    pub fn engines(&self) -> &[EngineEntry] {
        &self.engines
    }

    /// Number of engines.
    pub fn len(&self) -> usize {
        self.engines.len()
    }

    /// Whether the list has no engine.
    pub fn is_empty(&self) -> bool {
        self.engines.is_empty()
    }

    /// The id the next added engine receives (persisted so ids are not reused).
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// The engine with `id`.
    pub fn get(&self, id: EngineId) -> Option<&EngineEntry> {
        self.engines.iter().find(|e| e.id == id)
    }

    /// Position of `id` in the list.
    pub fn position(&self, id: EngineId) -> Option<usize> {
        self.engines.iter().position(|e| e.id == id)
    }

    /// The engine that owns `key`.
    pub fn hotkey_owner(&self, key: Hotkey) -> Option<EngineId> {
        self.hotkeys[key.index()]
    }

    /// The key of `id`.
    pub fn hotkey_of(&self, id: EngineId) -> Option<Hotkey> {
        Hotkey::ALL
            .into_iter()
            .find(|k| self.hotkeys[k.index()] == Some(id))
    }

    fn index_of(&self, id: EngineId) -> Result<usize, ListError> {
        self.position(id).ok_or(ListError::NotFound(id.0))
    }

    /// Appends an engine, without a key, and returns its id.
    pub fn add(&mut self, draft: EngineDraft) -> Result<EngineId, ListError> {
        if self.engines.len() >= MAX_ENGINES {
            return Err(ListError::TooManyEngines { limit: MAX_ENGINES });
        }
        let draft = draft.normalized()?;
        let id = EngineId(self.next_id);
        self.next_id += 1;
        self.engines.push(EngineEntry::from_draft(id, draft));
        Ok(id)
    }

    /// Replaces the editable fields of `id`; position, id and key stay.
    pub fn edit(&mut self, id: EngineId, draft: EngineDraft) -> Result<(), ListError> {
        let index = self.index_of(id)?;
        let draft = draft.normalized()?;
        self.engines[index] = EngineEntry::from_draft(id, draft);
        Ok(())
    }

    /// Appends a copy of `id` named `<name>-new`, without a key (Scid:
    /// `analysis.tcl:552-556`).
    pub fn copy(&mut self, id: EngineId) -> Result<EngineId, ListError> {
        let mut draft = self.get(id).ok_or(ListError::NotFound(id.0))?.to_draft();
        draft.name.push_str(COPY_SUFFIX);
        self.add(draft)
    }

    /// Removes `id` and frees its key. The other keys keep their engines:
    /// keys hold ids, so nothing is renumbered (D425).
    pub fn remove(&mut self, id: EngineId) -> Result<EngineEntry, ListError> {
        let index = self.index_of(id)?;
        for slot in &mut self.hotkeys {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        Ok(self.engines.remove(index))
    }

    /// Swaps `id` with the engine above. `Ok(false)` when it is the first.
    pub fn move_up(&mut self, id: EngineId) -> Result<bool, ListError> {
        let index = self.index_of(id)?;
        if index == 0 {
            return Ok(false);
        }
        self.engines.swap(index, index - 1);
        Ok(true)
    }

    /// Swaps `id` with the engine below. `Ok(false)` when it is the last.
    pub fn move_down(&mut self, id: EngineId) -> Result<bool, ListError> {
        let index = self.index_of(id)?;
        if index + 1 >= self.engines.len() {
            return Ok(false);
        }
        self.engines.swap(index, index + 1);
        Ok(true)
    }

    /// Gives `key` to `id` (taking it from its previous owner, and freeing
    /// the key `id` had), or frees the key `id` has when `key` is `None`.
    pub fn set_hotkey(&mut self, id: EngineId, key: Option<Hotkey>) -> Result<(), ListError> {
        self.index_of(id)?;
        for slot in &mut self.hotkeys {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        if let Some(key) = key {
            self.hotkeys[key.index()] = Some(id);
        }
        Ok(())
    }

    /// When no engine has a key, gives F2 to the first engine and F3 to the
    /// second (the defaults of the Scid, D427). Returns whether it did.
    pub fn assign_default_hotkeys(&mut self) -> bool {
        if self.engines.len() == 2
            && self.hotkeys[0].is_some()
            && self.hotkeys[1].is_none()
            && let Some(second) = self.engines.iter().find(|e| self.hotkey_of(e.id).is_none())
        {
            // The second engine added to a list whose first one holds F2.
            self.hotkeys[1] = Some(second.id);
            return true;
        }
        if self.hotkeys.iter().any(Option::is_some) || self.engines.is_empty() {
            return false;
        }
        for (slot, engine) in self.hotkeys.iter_mut().zip(&self.engines).take(2) {
            *slot = Some(engine.id);
        }
        true
    }

    /// Stores the options saved for `id`.
    pub fn set_options(
        &mut self,
        id: EngineId,
        options: Vec<(String, String)>,
    ) -> Result<(), ListError> {
        let index = self.index_of(id)?;
        let draft = EngineDraft {
            options,
            ..self.engines[index].to_draft()
        };
        self.edit(id, draft)
    }

    /// Records the start time of `id` (seconds since the Unix epoch).
    pub fn set_last_used(&mut self, id: EngineId, seconds: i64) -> Result<(), ListError> {
        let index = self.index_of(id)?;
        self.engines[index].last_used = Some(seconds);
        Ok(())
    }

    /// The next engine after `from` (or the first when `None` or not found)
    /// whose name starts with `initial`, ignoring case, wrapping around: the
    /// "type a letter to jump" of the Scid (`analysis.tcl:839-863`).
    pub fn find_by_initial(&self, from: Option<EngineId>, initial: char) -> Option<EngineId> {
        let n = self.engines.len();
        let start = from.and_then(|id| self.position(id)).map_or(0, |p| p + 1);
        let wanted: Vec<char> = initial.to_lowercase().collect();
        (0..n)
            .map(|k| &self.engines[(start + k) % n])
            .find(|e| {
                let first: Vec<char> = e
                    .name
                    .chars()
                    .next()
                    .into_iter()
                    .flat_map(char::to_lowercase)
                    .collect();
                first == wanted
            })
            .map(|e| e.id)
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;

    fn draft(name: &str) -> EngineDraft {
        EngineDraft::new(name, format!("/bin/{name}"))
    }

    fn list3() -> (EngineList, [EngineId; 3]) {
        let mut l = EngineList::new();
        let a = l.add(draft("alpha")).unwrap();
        let b = l.add(draft("beta")).unwrap();
        let c = l.add(draft("Gamma")).unwrap();
        (l, [a, b, c])
    }

    #[test]
    fn add_validates_and_normalizes() {
        let mut l = EngineList::new();
        assert_eq!(l.add(draft("  ")), Err(ValidationError::EmptyName.into()));
        assert_eq!(
            l.add(EngineDraft::new("x", " ")),
            Err(ValidationError::EmptyCommand.into())
        );
        let mut d = draft("  sf  ");
        d.elo = Some(0);
        d.multipv = 0;
        d.url = Some("  ".into());
        let id = l.add(d).unwrap();
        let e = l.get(id).unwrap();
        assert_eq!(
            (e.name.as_str(), e.elo, e.multipv, e.url.clone()),
            ("sf", None, 1, None)
        );
        let mut d = draft("hi");
        d.elo = Some(MAX_ELO + 1);
        assert_eq!(
            l.add(d),
            Err(ValidationError::EloOutOfRange(MAX_ELO + 1).into())
        );
    }

    #[test]
    fn limits_are_enforced() {
        let mut d = draft("x");
        d.args = vec![String::new(); MAX_ITEMS + 1];
        assert!(matches!(
            d.normalized(),
            Err(ValidationError::TooManyItems { field: "args", .. })
        ));
        let d = draft(&"n".repeat(MAX_TEXT_CHARS + 1));
        assert!(matches!(
            d.normalized(),
            Err(ValidationError::TextTooLong { field: "name", .. })
        ));
        let mut l = EngineList::new();
        for i in 0..MAX_ENGINES {
            l.add(draft(&format!("e{i}"))).unwrap();
        }
        assert_eq!(
            l.add(draft("one more")),
            Err(ListError::TooManyEngines { limit: MAX_ENGINES })
        );
    }

    #[test]
    fn ids_are_never_reused() {
        let (mut l, [a, b, c]) = list3();
        l.remove(c).unwrap();
        let d = l.add(draft("delta")).unwrap();
        assert!(d.0 > c.0 && d != a && d != b);
    }

    #[test]
    fn hotkeys_have_one_owner_and_survive_deletion_of_others() {
        let (mut l, [a, b, c]) = list3();
        l.set_hotkey(a, Some(Hotkey::F2)).unwrap();
        l.set_hotkey(b, Some(Hotkey::F3)).unwrap();
        l.set_hotkey(c, Some(Hotkey::F4)).unwrap();
        l.set_hotkey(c, Some(Hotkey::F2)).unwrap();
        assert_eq!(l.hotkey_owner(Hotkey::F2), Some(c));
        assert_eq!(l.hotkey_of(a), None);
        assert_eq!(l.hotkey_owner(Hotkey::F4), None);
        l.remove(a).unwrap();
        assert_eq!(l.hotkey_of(b), Some(Hotkey::F3));
        assert_eq!(l.hotkey_owner(Hotkey::F2), Some(c));
        l.remove(c).unwrap();
        assert_eq!(l.hotkey_owner(Hotkey::F2), None);
        assert_eq!(
            l.set_hotkey(EngineId(99), None),
            Err(ListError::NotFound(99))
        );
    }

    #[test]
    fn keys_follow_engines_when_moved() {
        let (mut l, [a, b, _]) = list3();
        l.set_hotkey(a, Some(Hotkey::F2)).unwrap();
        assert!(l.move_down(a).unwrap());
        assert_eq!(l.position(a), Some(1));
        assert_eq!(l.position(b), Some(0));
        assert_eq!(l.hotkey_of(a), Some(Hotkey::F2));
        assert!(!l.move_up(b).unwrap());
        assert!(l.move_up(a).unwrap());
    }

    #[test]
    fn copy_appends_with_suffix_and_no_key() {
        let (mut l, [a, ..]) = list3();
        l.set_hotkey(a, Some(Hotkey::F2)).unwrap();
        let copy = l.copy(a).unwrap();
        assert_eq!(l.get(copy).unwrap().name, "alpha-new");
        assert_eq!(l.position(copy), Some(3));
        assert_eq!(l.hotkey_of(copy), None);
    }

    #[test]
    fn edit_keeps_identity() {
        let (mut l, [a, ..]) = list3();
        l.set_hotkey(a, Some(Hotkey::F3)).unwrap();
        l.edit(a, draft("renamed")).unwrap();
        assert_eq!(l.get(a).unwrap().name, "renamed");
        assert_eq!(l.hotkey_of(a), Some(Hotkey::F3));
        assert!(l.edit(a, draft("")).is_err());
        assert_eq!(l.get(a).unwrap().name, "renamed");
    }

    #[test]
    fn default_hotkeys_only_on_a_list_without_keys() {
        let (mut l, [a, b, c]) = list3();
        assert!(l.assign_default_hotkeys());
        assert_eq!(l.hotkey_owner(Hotkey::F2), Some(a));
        assert_eq!(l.hotkey_owner(Hotkey::F3), Some(b));
        assert_eq!(l.hotkey_owner(Hotkey::F4), None);
        assert!(!l.assign_default_hotkeys());
        assert_eq!(l.hotkey_of(c), None);
        assert!(!EngineList::new().assign_default_hotkeys());
    }

    #[test]
    fn initial_search_wraps_and_ignores_case() {
        let mut l = EngineList::new();
        let s1 = l.add(draft("Stockfish")).unwrap();
        let k = l.add(draft("Komodo")).unwrap();
        let s2 = l.add(draft("stash")).unwrap();
        assert_eq!(l.find_by_initial(None, 's'), Some(s1));
        assert_eq!(l.find_by_initial(Some(s1), 'S'), Some(s2));
        assert_eq!(l.find_by_initial(Some(s2), 's'), Some(s1));
        assert_eq!(l.find_by_initial(Some(s1), 'k'), Some(k));
        assert_eq!(l.find_by_initial(None, 'z'), None);
        assert_eq!(EngineList::new().find_by_initial(None, 'a'), None);
    }

    #[test]
    fn set_options_and_last_used() {
        let (mut l, [a, ..]) = list3();
        l.set_options(a, vec![("Hash".into(), "128".into())])
            .unwrap();
        l.set_last_used(a, 1_700_000_000).unwrap();
        let e = l.get(a).unwrap();
        assert_eq!(e.options, [("Hash".to_owned(), "128".to_owned())]);
        assert_eq!(e.last_used, Some(1_700_000_000));
    }
}
