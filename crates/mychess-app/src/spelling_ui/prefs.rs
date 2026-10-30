//! The `[player_tools]` table of `preferences.toml` (spec M6-3, slice b): the
//! spelling file, the photos, the players looked up, and the last values of
//! the player finder, the tournament finder, the name checker and "Add Elo".
//!
//! The spec called the table `[players]`; that name already belongs to the
//! player strips (M2-5), so the keys live in `[player_tools]` and its
//! sub-tables (D3460).

use serde::{Serialize, Serializer};
use toml::{Table, Value};

use crate::panel_calc::named_enum;
use crate::prefs::PrefsIssue;

/// The most players kept in the history of "Player information" (D3321).
pub const HISTORY_MAX: usize = 10;
/// The longest path or name kept in the preferences.
pub(crate) const MAX_TEXT_CHARS: usize = 1024;
/// The largest number of games or players of the finder windows.
pub(crate) const MAX_GAMES: u32 = 1_000_000;
/// The largest list size of the finders (the Scid's own limit).
pub const MAX_LIST_SIZE: u32 = 100_000;
/// The default list size of the finders (D3312).
pub const DEFAULT_LIST_SIZE: u32 = 1000;
/// The default start date of the tournament finder (`defaults` of
/// `tourney.tcl`).
pub const DEFAULT_TOURNAMENT_START: &str = "1960.??.??";

named_enum! {
    /// The column the player finder is ordered by (the Scid's `PListSort`).
    pub enum PlayerSortKey {
        /// By name, case-insensitive (the default).
        #[default]
        Name => "name",
        /// By number of games, descending.
        Games => "games",
        /// By the year of the oldest game, ascending.
        Oldest => "oldest",
        /// By the year of the newest game, descending (D3311).
        Newest => "newest",
        /// By the highest Elo, descending.
        Elo => "elo",
        /// By country code, ascending.
        Country => "country",
    }
}

named_enum! {
    /// The column the tournament finder is ordered by (`TmtSort`).
    pub enum TournamentSortKey {
        /// By date of the first game, newest first (the default).
        #[default]
        Date => "date",
        /// By number of players, descending.
        Players => "players",
        /// By number of games, descending.
        Games => "games",
        /// By mean Elo, descending.
        Elo => "elo",
        /// By event name.
        Event => "event",
        /// By site name.
        Site => "site",
        /// By the name of the winner.
        Winner => "winner",
    }
}

impl Serialize for PlayerSortKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl Serialize for TournamentSortKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// The last values of the player finder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FinderPrefs {
    /// Fewest games (the window's default is 1).
    pub games_min: u32,
    /// Most games (default 100 000).
    pub games_max: u32,
    /// Lowest Elo (default 0).
    pub elo_min: u16,
    /// Highest Elo (default 4000, D3312).
    pub elo_max: u16,
    /// List size; 0 = no limit.
    pub limit: u32,
    /// The order.
    pub sort: PlayerSortKey,
    /// The direction when the user flipped it; `""` = the natural one of the
    /// column, `"asc"` or `"desc"`.
    pub direction: String,
}

impl Default for FinderPrefs {
    fn default() -> FinderPrefs {
        FinderPrefs {
            games_min: 1,
            games_max: 100_000,
            elo_min: 0,
            elo_max: mychess_core::MAX_ELO,
            limit: DEFAULT_LIST_SIZE,
            sort: PlayerSortKey::Name,
            direction: String::new(),
        }
    }
}

/// The last values of the tournament finder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TournamentPrefs {
    /// The start date text (`1960.??.??`).
    pub start: String,
    /// Fewest players (default 4).
    pub min_players: u32,
    /// Fewest games (default 10).
    pub min_games: u32,
    /// List size; 0 = no limit.
    pub limit: u32,
    /// The order.
    pub sort: TournamentSortKey,
}

impl Default for TournamentPrefs {
    fn default() -> TournamentPrefs {
        TournamentPrefs {
            start: DEFAULT_TOURNAMENT_START.to_owned(),
            min_players: 4,
            min_games: 10,
            limit: DEFAULT_LIST_SIZE,
            sort: TournamentSortKey::Date,
        }
    }
}

/// The options of the name checker window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct SpellcheckPrefs {
    /// Also propose names that are only a surname.
    pub surnames: bool,
    /// Also list the ambiguous corrections.
    pub ambiguous: bool,
}

/// The options of "Add Elo" (`start.tcl` 1546-1547: overwrite off, only the
/// filter on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AddEloPrefs {
    /// Replace the ratings that are already there.
    pub overwrite: bool,
    /// Only the games of the filter.
    pub only_filter: bool,
}

impl Default for AddEloPrefs {
    fn default() -> AddEloPrefs {
        AddEloPrefs {
            overwrite: false,
            only_filter: true,
        }
    }
}

/// Everything of `[player_tools]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlayerToolsPrefs {
    /// The spelling file; empty = `spelling.ssp` (or `.ssp.gz`) in the data
    /// directory (D3335).
    pub spelling_file: String,
    /// Load it when the program starts (`loadAtStart(spell)`).
    pub load_at_start: bool,
    /// An extra folder of photos; the data directory's `photos` is always
    /// searched.
    pub photos_dir: String,
    /// Show the photos in the game information area (`gameInfo(photos)`).
    pub show_photos: bool,
    /// The players looked up last, newest first (D3321).
    pub history: Vec<String>,
    /// The player finder.
    pub finder: FinderPrefs,
    /// The tournament finder.
    pub tournaments: TournamentPrefs,
    /// The name checker.
    pub spellcheck: SpellcheckPrefs,
    /// Add Elo.
    pub add_elo: AddEloPrefs,
}

impl Default for PlayerToolsPrefs {
    fn default() -> PlayerToolsPrefs {
        PlayerToolsPrefs {
            spelling_file: String::new(),
            load_at_start: true,
            photos_dir: String::new(),
            show_photos: false,
            history: Vec::new(),
            finder: FinderPrefs::default(),
            tournaments: TournamentPrefs::default(),
            spellcheck: SpellcheckPrefs::default(),
            add_elo: AddEloPrefs::default(),
        }
    }
}

fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT_CHARS)
        .collect()
}

impl PlayerToolsPrefs {
    /// The preferences with every number inside its range, the history
    /// without repeats or blanks and at most [`HISTORY_MAX`] long.
    #[must_use]
    pub fn sanitized(mut self) -> PlayerToolsPrefs {
        self.spelling_file = clean_text(&self.spelling_file);
        self.photos_dir = clean_text(&self.photos_dir);
        let mut history: Vec<String> = Vec::new();
        for name in self.history.drain(..) {
            let name = clean_text(name.trim());
            if !name.is_empty() && !history.contains(&name) && history.len() < HISTORY_MAX {
                history.push(name);
            }
        }
        self.history = history;
        let f = &mut self.finder;
        f.games_min = f.games_min.min(MAX_GAMES);
        f.games_max = f.games_max.clamp(f.games_min.max(1), MAX_GAMES);
        f.elo_max = f.elo_max.min(mychess_core::MAX_ELO);
        f.elo_min = f.elo_min.min(f.elo_max);
        f.limit = f.limit.min(MAX_LIST_SIZE);
        if !matches!(f.direction.as_str(), "" | "asc" | "desc") {
            f.direction.clear();
        }
        let t = &mut self.tournaments;
        t.start = clean_text(&t.start);
        if !mychess_core::PartialDate::is_valid_str(&t.start) {
            DEFAULT_TOURNAMENT_START.clone_into(&mut t.start);
        }
        t.min_players = t.min_players.clamp(1, 1999);
        t.min_games = t.min_games.min(MAX_GAMES);
        t.limit = t.limit.min(MAX_LIST_SIZE);
        self
    }

    /// Moves `name` to the front of the history.
    pub(crate) fn remember(&mut self, name: &str) {
        let name = clean_text(name.trim());
        if name.is_empty() {
            return;
        }
        self.history.retain(|n| *n != name);
        self.history.insert(0, name);
        self.history.truncate(HISTORY_MAX);
    }
}

// ---- reading ----

struct Reader<'a> {
    name: String,
    table: Option<&'a Table>,
}

impl<'a> Reader<'a> {
    fn root(root: &'a Table, name: &str, issues: &mut Vec<PrefsIssue>) -> Reader<'a> {
        Reader::of(root.get(name), name.to_owned(), issues)
    }

    fn child(&self, name: &str, issues: &mut Vec<PrefsIssue>) -> Reader<'a> {
        let value = self.table.and_then(|t| t.get(name));
        Reader::of(value, format!("{}.{name}", self.name), issues)
    }

    fn of(value: Option<&'a Value>, name: String, issues: &mut Vec<PrefsIssue>) -> Reader<'a> {
        let table = match value {
            None => None,
            Some(Value::Table(table)) => Some(table),
            Some(_) => {
                issues.push(PrefsIssue::InvalidValue { key: name.clone() });
                None
            }
        };
        Reader { name, table }
    }

    fn invalid(&self, key: &str, issues: &mut Vec<PrefsIssue>) {
        issues.push(PrefsIssue::InvalidValue {
            key: format!("{}.{key}", self.name),
        });
    }

    fn bool(&self, key: &str, default: bool, issues: &mut Vec<PrefsIssue>) -> bool {
        match self.table.and_then(|t| t.get(key)) {
            None => default,
            Some(Value::Boolean(b)) => *b,
            Some(_) => {
                self.invalid(key, issues);
                default
            }
        }
    }

    fn text(&self, key: &str, default: &str, issues: &mut Vec<PrefsIssue>) -> String {
        match self.table.and_then(|t| t.get(key)) {
            None => default.to_owned(),
            Some(Value::String(s)) => s.clone(),
            Some(_) => {
                self.invalid(key, issues);
                default.to_owned()
            }
        }
    }

    fn number(&self, key: &str, max: u32, default: u32, issues: &mut Vec<PrefsIssue>) -> u32 {
        match self.table.and_then(|t| t.get(key)) {
            None => default,
            Some(Value::Integer(n)) => {
                let wanted = u32::try_from((*n).max(0)).unwrap_or(u32::MAX);
                if *n < 0 || wanted > max {
                    issues.push(PrefsIssue::Clamped {
                        key: format!("{}.{key}", self.name),
                    });
                }
                wanted.min(max)
            }
            Some(_) => {
                self.invalid(key, issues);
                default
            }
        }
    }

    fn choice<T: Copy>(
        &self,
        key: &str,
        default: T,
        parse: fn(&str) -> Option<T>,
        issues: &mut Vec<PrefsIssue>,
    ) -> T {
        match self.table.and_then(|t| t.get(key)) {
            None => default,
            Some(Value::String(s)) => parse(s).unwrap_or_else(|| {
                self.invalid(key, issues);
                default
            }),
            Some(_) => {
                self.invalid(key, issues);
                default
            }
        }
    }

    fn list(&self, key: &str, issues: &mut Vec<PrefsIssue>) -> Vec<String> {
        match self.table.and_then(|t| t.get(key)) {
            None => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            Some(_) => {
                self.invalid(key, issues);
                Vec::new()
            }
        }
    }
}

/// Reads the `[player_tools]` table: a wrong value only resets its own
/// field.
pub(crate) fn read_player_tools(
    root: &Table,
    defaults: &PlayerToolsPrefs,
    issues: &mut Vec<PrefsIssue>,
) -> PlayerToolsPrefs {
    let sec = Reader::root(root, "player_tools", issues);
    let finder = sec.child("finder", issues);
    let tournaments = sec.child("tournaments", issues);
    let spellcheck = sec.child("spellcheck", issues);
    let add_elo = sec.child("add_elo", issues);
    let df = &defaults.finder;
    let dt = &defaults.tournaments;
    let prefs = PlayerToolsPrefs {
        spelling_file: sec.text("spelling_file", &defaults.spelling_file, issues),
        load_at_start: sec.bool("load_at_start", defaults.load_at_start, issues),
        photos_dir: sec.text("photos_dir", &defaults.photos_dir, issues),
        show_photos: sec.bool("show_photos", defaults.show_photos, issues),
        history: sec.list("history", issues),
        finder: FinderPrefs {
            games_min: finder.number("games_min", MAX_GAMES, df.games_min, issues),
            games_max: finder.number("games_max", MAX_GAMES, df.games_max, issues),
            elo_min: u16::try_from(finder.number(
                "elo_min",
                u32::from(mychess_core::MAX_ELO),
                u32::from(df.elo_min),
                issues,
            ))
            .unwrap_or(df.elo_min),
            elo_max: u16::try_from(finder.number(
                "elo_max",
                u32::from(mychess_core::MAX_ELO),
                u32::from(df.elo_max),
                issues,
            ))
            .unwrap_or(df.elo_max),
            limit: finder.number("limit", MAX_LIST_SIZE, df.limit, issues),
            sort: finder.choice("sort", df.sort, PlayerSortKey::from_name, issues),
            direction: finder.text("direction", &df.direction, issues),
        },
        tournaments: TournamentPrefs {
            start: tournaments.text("start", &dt.start, issues),
            min_players: tournaments.number("min_players", 1999, dt.min_players, issues),
            min_games: tournaments.number("min_games", MAX_GAMES, dt.min_games, issues),
            limit: tournaments.number("limit", MAX_LIST_SIZE, dt.limit, issues),
            sort: tournaments.choice("sort", dt.sort, TournamentSortKey::from_name, issues),
        },
        spellcheck: SpellcheckPrefs {
            surnames: spellcheck.bool("surnames", defaults.spellcheck.surnames, issues),
            ambiguous: spellcheck.bool("ambiguous", defaults.spellcheck.ambiguous, issues),
        },
        add_elo: AddEloPrefs {
            overwrite: add_elo.bool("overwrite", defaults.add_elo.overwrite, issues),
            only_filter: add_elo.bool("only_filter", defaults.add_elo.only_filter, issues),
        },
    };
    let sane = prefs.clone().sanitized();
    if sane.tournaments.start != prefs.tournaments.start {
        tournaments.invalid("start", issues);
    }
    sane
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Preferences, load_preferences, render_preferences};

    #[test]
    fn defaults_follow_the_spec() {
        let p = PlayerToolsPrefs::default();
        assert!(p.load_at_start && !p.show_photos);
        assert!(p.spelling_file.is_empty() && p.photos_dir.is_empty());
        assert_eq!((p.finder.games_min, p.finder.games_max), (1, 100_000));
        assert_eq!((p.finder.elo_min, p.finder.elo_max), (0, 4000));
        assert_eq!(p.finder.limit, 1000);
        assert_eq!(p.tournaments.start, "1960.??.??");
        assert_eq!(
            (p.tournaments.min_players, p.tournaments.min_games),
            (4, 10)
        );
        assert!(!p.spellcheck.ambiguous && !p.spellcheck.surnames);
        assert!(!p.add_elo.overwrite && p.add_elo.only_filter);
    }

    #[test]
    fn the_file_round_trips_and_keeps_the_player_strips_table() {
        let tools = PlayerToolsPrefs {
            spelling_file: "/data/spelling.ssp".to_owned(),
            load_at_start: false,
            photos_dir: "/data/photos".to_owned(),
            show_photos: true,
            history: vec!["Alfa, A".to_owned(), "Beta, B".to_owned()],
            finder: FinderPrefs {
                games_min: 5,
                games_max: 900,
                elo_min: 1500,
                elo_max: 2800,
                limit: 200,
                sort: PlayerSortKey::Elo,
                direction: "asc".to_owned(),
            },
            tournaments: TournamentPrefs {
                start: "1990.05.??".to_owned(),
                min_players: 6,
                min_games: 15,
                limit: 50,
                sort: TournamentSortKey::Winner,
            },
            spellcheck: SpellcheckPrefs {
                surnames: true,
                ambiguous: true,
            },
            add_elo: AddEloPrefs {
                overwrite: true,
                only_filter: false,
            },
        };
        let prefs = Preferences {
            player_tools: Box::new(tools.clone()),
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[player_tools"), "{text}");
        assert!(text.contains("[players]"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(*loaded.prefs.player_tools, tools);
        assert_eq!(loaded.prefs.players, Preferences::default().players);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn a_file_without_the_table_has_the_defaults() {
        let loaded = load_preferences("[board]\nanimation_ms = 100\n");
        assert_eq!(*loaded.prefs.player_tools, PlayerToolsPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn bad_values_fall_back_one_by_one() {
        let loaded = load_preferences(
            "[player_tools]\nload_at_start = 3\nshow_photos = true\nhistory = \"x\"\n\
             [player_tools.finder]\nsort = \"nonsense\"\ngames_min = 7\nelo_max = 99999\n\
             [player_tools.tournaments]\nstart = \"junk!\"\nmin_players = 0\n",
        );
        let p = &loaded.prefs.player_tools;
        assert!(p.load_at_start, "the default");
        assert!(p.show_photos, "a good value next to a bad one");
        assert!(p.history.is_empty());
        assert_eq!(p.finder.sort, PlayerSortKey::Name);
        assert_eq!(p.finder.games_min, 7);
        assert_eq!(p.finder.elo_max, 4000, "limited");
        assert_eq!(p.tournaments.start, DEFAULT_TOURNAMENT_START);
        assert_eq!(p.tournaments.min_players, 1, "limited to 1");
        assert!(loaded.issues.len() >= 4, "{:?}", loaded.issues);
        let loaded = load_preferences("player_tools = 1\n");
        assert_eq!(*loaded.prefs.player_tools, PlayerToolsPrefs::default());
        assert_eq!(loaded.issues.len(), 1);
    }

    #[test]
    fn the_history_keeps_ten_names_newest_first_without_repeats() {
        let mut p = PlayerToolsPrefs::default();
        for i in 0..14 {
            p.remember(&format!("Player {i}"));
        }
        assert_eq!(p.history.len(), HISTORY_MAX);
        assert_eq!(p.history[0], "Player 13");
        p.remember("Player 9");
        assert_eq!(p.history[0], "Player 9");
        assert_eq!(p.history.iter().filter(|n| *n == "Player 9").count(), 1);
        p.remember("   ");
        assert_eq!(p.history.len(), HISTORY_MAX);
    }

    #[test]
    fn sanitizing_orders_the_ranges() {
        let p = PlayerToolsPrefs {
            finder: FinderPrefs {
                games_min: 50,
                games_max: 10,
                elo_min: 3000,
                elo_max: 2000,
                limit: 999_999,
                direction: "up".to_owned(),
                ..FinderPrefs::default()
            },
            ..PlayerToolsPrefs::default()
        }
        .sanitized();
        assert_eq!(p.finder.games_max, 50);
        assert_eq!(p.finder.elo_min, 2000);
        assert_eq!(p.finder.limit, MAX_LIST_SIZE);
        assert!(p.finder.direction.is_empty());
    }
}
