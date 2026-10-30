//! The TOML format of a saved search (D636, D637, D638).

use std::collections::BTreeMap;

use mychess_core::{Eco, PartialDate};
use serde::{Deserialize, Serialize};

use super::{SavedSearch, SavedSearchError, validate_created, validate_name};
use crate::filter::FilterOp;
use crate::search::{
    EcoRange, FlagFilter, GameEndKind, HeaderSearch, MinMax, ResultSet, TitleSet, Tri,
};
use crate::si4::GameFlags;

/// The `format` key of every file.
pub(super) const FORMAT_NAME: &str = "mychess-saved-search";
/// The version this code writes and the newest it reads.
pub(super) const FORMAT_VERSION: u32 = 1;

/// The 22 flags: TOML key, flag, name of Scid's `sHeaderFlags(...)` index.
pub(super) const FLAGS: [(&str, GameFlags, &str); 22] = [
    ("start", GameFlags::START, "StdStart"),
    ("promotion", GameFlags::PROMOTION, "Promotions"),
    ("under_promotion", GameFlags::UNDER_PROMOTION, "UnderPromo"),
    ("delete", GameFlags::DELETE, "DeleteFlag"),
    ("white_opening", GameFlags::WHITE_OPENING, "WhiteOpFlag"),
    ("black_opening", GameFlags::BLACK_OPENING, "BlackOpFlag"),
    ("middlegame", GameFlags::MIDDLEGAME, "MiddlegameFlag"),
    ("endgame", GameFlags::ENDGAME, "EndgameFlag"),
    ("novelty", GameFlags::NOVELTY, "NoveltyFlag"),
    ("pawn_structure", GameFlags::PAWN_STRUCTURE, "PawnFlag"),
    ("tactics", GameFlags::TACTICS, "TacticsFlag"),
    ("kingside", GameFlags::KINGSIDE, "KsideFlag"),
    ("queenside", GameFlags::QUEENSIDE, "QsideFlag"),
    ("brilliancy", GameFlags::BRILLIANCY, "BrilliancyFlag"),
    ("blunder", GameFlags::BLUNDER, "BlunderFlag"),
    ("user", GameFlags::USER, "UserFlag"),
    ("custom1", GameFlags::CUSTOM1, "CustomFlag1"),
    ("custom2", GameFlags::CUSTOM2, "CustomFlag2"),
    ("custom3", GameFlags::CUSTOM3, "CustomFlag3"),
    ("custom4", GameFlags::CUSTOM4, "CustomFlag4"),
    ("custom5", GameFlags::CUSTOM5, "CustomFlag5"),
    ("custom6", GameFlags::CUSTOM6, "CustomFlag6"),
];

/// The eight title names (the last one is "no title").
pub(super) const TITLES: [(&str, TitleSet); 8] = [
    ("gm", TitleSet::GM),
    ("im", TitleSet::IM),
    ("fm", TitleSet::FM),
    ("cm", TitleSet::CM),
    ("wgm", TitleSet::WGM),
    ("wim", TitleSet::WIM),
    ("wfm", TitleSet::WFM),
    ("none", TitleSet::NONE),
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDto {
    format: String,
    version: u32,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<String>,
    #[serde(default)]
    search: SearchDto,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeDto<T> {
    min: T,
    max: T,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EcoDto {
    min: String,
    max: String,
    include_none: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultsDto {
    white: bool,
    draw: bool,
    black: bool,
    none: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct SearchDto {
    white: String,
    black: String,
    event: String,
    site: String,
    round: String,
    date_min: String,
    date_max: String,
    results: ResultsDto,
    white_elo: RangeDto<u16>,
    black_elo: RangeDto<u16>,
    elo_diff: RangeDto<i32>,
    white_titles: Vec<String>,
    black_titles: Vec<String>,
    end_white_to_move: bool,
    end_black_to_move: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    eco: Option<EcoDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    length: Option<RangeDto<u16>>,
    game_number: RangeDto<i64>,
    ignore_colors: bool,
    op: String,
    extra_tag: String,
    extra_tag_value: String,
    pgn_text: Vec<String>,
    ignore_case: bool,
    game_end: String,
    pre_comment: bool,
    post_comment: bool,
    flags: BTreeMap<String, String>,
}

impl Default for SearchDto {
    fn default() -> SearchDto {
        SearchDto::from_search(&HeaderSearch::default())
    }
}

fn tri_name(tri: Tri) -> &'static str {
    match tri {
        Tri::Yes => "yes",
        Tri::No => "no",
        Tri::Both => "both",
    }
}

fn titles_to_names(set: TitleSet) -> Vec<String> {
    TITLES
        .iter()
        .filter(|(_, bit)| set.contains(*bit))
        .map(|(name, _)| (*name).to_owned())
        .collect()
}

impl SearchDto {
    fn from_search(s: &HeaderSearch) -> SearchDto {
        let flags = FLAGS
            .iter()
            .filter_map(|(key, flag, _)| {
                let tri = s.flags.get(*flag);
                (tri != Tri::Both).then(|| ((*key).to_owned(), tri_name(tri).to_owned()))
            })
            .collect();
        SearchDto {
            white: s.white.clone(),
            black: s.black.clone(),
            event: s.event.clone(),
            site: s.site.clone(),
            round: s.round.clone(),
            date_min: s.date.min.to_string(),
            date_max: s.date.max.to_string(),
            results: ResultsDto {
                white: s.results.white,
                draw: s.results.draw,
                black: s.results.black,
                none: s.results.none,
            },
            white_elo: RangeDto {
                min: s.white_elo.min,
                max: s.white_elo.max,
            },
            black_elo: RangeDto {
                min: s.black_elo.min,
                max: s.black_elo.max,
            },
            elo_diff: RangeDto {
                min: s.elo_diff.min,
                max: s.elo_diff.max,
            },
            white_titles: titles_to_names(s.white_titles),
            black_titles: titles_to_names(s.black_titles),
            end_white_to_move: s.end_white_to_move,
            end_black_to_move: s.end_black_to_move,
            eco: s.eco.map(|e| EcoDto {
                min: e.min.to_string(),
                max: e.max.to_string(),
                include_none: e.include_none,
            }),
            length: s.length.map(|l| RangeDto {
                min: l.min,
                max: l.max,
            }),
            game_number: RangeDto {
                min: s.game_number.min,
                max: s.game_number.max,
            },
            ignore_colors: s.ignore_colors,
            op: match s.op {
                FilterOp::And => "and",
                FilterOp::Or => "or",
                FilterOp::Reset => "reset",
            }
            .to_owned(),
            extra_tag: s.extra_tag.clone(),
            extra_tag_value: s.extra_tag_value.clone(),
            pgn_text: s.pgn_text.clone(),
            ignore_case: s.ignore_case,
            game_end: match s.game_end {
                GameEndKind::Any => "any",
                GameEndKind::Checkmate => "checkmate",
                GameEndKind::Stalemate => "stalemate",
            }
            .to_owned(),
            pre_comment: s.pre_comment,
            post_comment: s.post_comment,
            flags,
        }
    }

    fn into_search(self) -> Result<HeaderSearch, SavedSearchError> {
        let mut flags = FlagFilter::default();
        for (key, value) in &self.flags {
            let Some((_, flag, _)) = FLAGS.iter().find(|(k, _, _)| k == key) else {
                return Err(invalid(&format!("flags.{key}"), key, "unknown flag"));
            };
            let tri = match value.as_str() {
                "yes" => Tri::Yes,
                "no" => Tri::No,
                "both" => Tri::Both,
                _ => {
                    return Err(invalid(
                        &format!("flags.{key}"),
                        value,
                        "expected yes or no",
                    ));
                }
            };
            flags.set(*flag, tri);
        }
        let eco = match self.eco {
            None => None,
            Some(e) => Some(EcoRange {
                min: e
                    .min
                    .parse::<Eco>()
                    .map_err(|_| invalid("eco.min", &e.min, "not an ECO code"))?,
                max: e
                    .max
                    .parse::<Eco>()
                    .map_err(|_| invalid("eco.max", &e.max, "not an ECO code"))?,
                include_none: e.include_none,
            }),
        };
        Ok(HeaderSearch {
            white: self.white,
            black: self.black,
            event: self.event,
            site: self.site,
            round: self.round,
            date: MinMax::new(
                parse_date("date_min", &self.date_min)?,
                parse_date("date_max", &self.date_max)?,
            ),
            results: ResultSet {
                white: self.results.white,
                draw: self.results.draw,
                black: self.results.black,
                none: self.results.none,
            },
            white_elo: MinMax::new(self.white_elo.min, self.white_elo.max),
            black_elo: MinMax::new(self.black_elo.min, self.black_elo.max),
            elo_diff: MinMax::new(self.elo_diff.min, self.elo_diff.max),
            white_titles: names_to_titles("white_titles", &self.white_titles)?,
            black_titles: names_to_titles("black_titles", &self.black_titles)?,
            end_white_to_move: self.end_white_to_move,
            end_black_to_move: self.end_black_to_move,
            eco,
            length: self.length.map(|l| MinMax::new(l.min, l.max)),
            game_number: MinMax::new(self.game_number.min, self.game_number.max),
            ignore_colors: self.ignore_colors,
            op: match self.op.as_str() {
                "and" => FilterOp::And,
                "or" => FilterOp::Or,
                "reset" => FilterOp::Reset,
                other => return Err(invalid("op", other, "expected and, or or reset")),
            },
            flags,
            extra_tag: self.extra_tag,
            extra_tag_value: self.extra_tag_value,
            pgn_text: self.pgn_text,
            ignore_case: self.ignore_case,
            game_end: match self.game_end.as_str() {
                "any" => GameEndKind::Any,
                "checkmate" => GameEndKind::Checkmate,
                "stalemate" => GameEndKind::Stalemate,
                other => {
                    return Err(invalid(
                        "game_end",
                        other,
                        "expected any, checkmate or stalemate",
                    ));
                }
            },
            pre_comment: self.pre_comment,
            post_comment: self.post_comment,
        })
    }
}

/// An [`SavedSearchError::InvalidValue`] with the value clipped (D638).
pub(super) fn invalid(field: &str, value: &str, reason: &str) -> SavedSearchError {
    SavedSearchError::InvalidValue {
        field: field.to_owned(),
        value: value.chars().take(64).collect(),
        reason: reason.to_owned(),
    }
}

fn parse_date(field: &str, text: &str) -> Result<PartialDate, SavedSearchError> {
    text.parse::<PartialDate>()
        .map_err(|_| invalid(field, text, "not a date (yyyy.mm.dd, ? for unknown)"))
}

fn names_to_titles(field: &str, names: &[String]) -> Result<TitleSet, SavedSearchError> {
    let mut set = TitleSet::empty();
    for name in names {
        let Some((_, bit)) = TITLES.iter().find(|(n, _)| n == name) else {
            return Err(invalid(field, name, "unknown title"));
        };
        set |= *bit;
    }
    Ok(set)
}

/// Writes the TOML text of `saved` (the caller has validated it).
pub(super) fn to_toml(saved: &SavedSearch) -> Result<String, SavedSearchError> {
    validate_name(&saved.name)?;
    if let Some(created) = &saved.created {
        validate_created(created)?;
    }
    let dto = FileDto {
        format: FORMAT_NAME.to_owned(),
        version: FORMAT_VERSION,
        name: saved.name.clone(),
        created: saved.created.clone(),
        search: SearchDto::from_search(&saved.search),
    };
    toml::to_string(&dto).map_err(|e| SavedSearchError::Toml {
        message: e.to_string(),
    })
}

/// Reads the TOML text of a saved search. The format name and version are
/// checked before the rest so that a file from a newer version reports
/// [`SavedSearchError::UnsupportedVersion`] instead of a field error.
pub(super) fn from_toml(text: &str) -> Result<SavedSearch, SavedSearchError> {
    let table: toml::Table = toml::from_str(text).map_err(|e| SavedSearchError::Toml {
        message: e.to_string(),
    })?;
    match table.get("format") {
        Some(toml::Value::String(s)) if s == FORMAT_NAME => {}
        _ => return Err(SavedSearchError::WrongFormat),
    }
    match table.get("version") {
        Some(toml::Value::Integer(v)) if *v == i64::from(FORMAT_VERSION) => {}
        Some(toml::Value::Integer(v)) => {
            return Err(SavedSearchError::UnsupportedVersion { found: *v });
        }
        _ => return Err(SavedSearchError::WrongFormat),
    }
    let mut table = table;
    match table.get("kind") {
        None => {}
        Some(toml::Value::String(k)) if k == "header" => {
            table.remove("kind");
        }
        Some(toml::Value::String(k)) => {
            return Err(SavedSearchError::WrongKind { found: k.clone() });
        }
        Some(_) => return Err(invalid("kind", "", "expected a string")),
    }
    let dto: FileDto = table.try_into().map_err(|e| SavedSearchError::Toml {
        message: e.to_string(),
    })?;
    validate_name(&dto.name)?;
    if let Some(created) = &dto.created {
        validate_created(created)?;
    }
    Ok(SavedSearch {
        name: dto.name,
        created: dto.created,
        search: dto.search.into_search()?,
    })
}
