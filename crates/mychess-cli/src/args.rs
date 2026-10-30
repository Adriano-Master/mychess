//! The command line (`clap` derive).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use mychess_db::si4::{SortKey, SortKeyError};

/// MyChess database tools: the equivalents of `pgnscid`, `scmerge` and `scidt`.
#[derive(Debug, Parser)]
#[command(name = "mychess-cli", version, about, arg_required_else_help = true)]
pub struct Cli {
    /// Show only the results, warnings and errors (no progress).
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a database from a PGN file (pgnscid).
    Import(ImportArgs),
    /// Merge databases into a new one (scmerge).
    Merge(MergeArgs),
    /// Show general information, compactness and name statistics (scidt -i, -c, -n).
    Info(InfoArgs),
    /// List the names of a kind, optionally by prefix (scidt -p, -e, -s, -r).
    Names(NamesArgs),
    /// Change the description of a database (scidt -d).
    Describe(DescribeArgs),
    /// Sort the index of a database (scidt -S, -R).
    Sort(SortArgs),
    /// Compact the games or the names of a database (scidt -C, -N).
    Compact(CompactArgs),
    /// Write the games of a database to a PGN file.
    Export(ExportArgs),
}

/// Arguments of `import`.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Overwrite an existing database.
    #[arg(short, long)]
    pub force: bool,
    /// Ignore the text between games (the default keeps it as a comment).
    #[arg(short = 'x', long)]
    pub ignore_pregame: bool,
    /// The PGN file (`.pgn` or `.pgn.gz`).
    pub pgn: PathBuf,
    /// The database; defaults to the name of the PGN without `.gz` and `.pgn`.
    pub base: Option<PathBuf>,
}

/// Arguments of `merge`.
#[derive(Debug, Args)]
pub struct MergeArgs {
    /// The new database (must not exist).
    pub target: PathBuf,
    /// The databases to merge; `-` reads their names from standard input, one per line.
    #[arg(required = true)]
    pub sources: Vec<PathBuf>,
}

/// Arguments of `info`.
#[derive(Debug, Args)]
pub struct InfoArgs {
    /// Also show the current and compacted size of the game file.
    #[arg(short, long)]
    pub compact: bool,
    /// Also show statistics of the names.
    #[arg(short, long)]
    pub names: bool,
    /// The database.
    pub base: PathBuf,
}

/// A kind of name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKindArg {
    /// Players.
    Player,
    /// Events.
    Event,
    /// Sites.
    Site,
    /// Rounds.
    Round,
}

impl NameKindArg {
    const ALL: [(&'static str, NameKindArg); 4] = [
        ("player", NameKindArg::Player),
        ("event", NameKindArg::Event),
        ("site", NameKindArg::Site),
        ("round", NameKindArg::Round),
    ];
}

/// Reads a kind of name: the full name or a unique prefix, in any letter case
/// (spec rule 4, D633).
fn parse_name_kind(text: &str) -> Result<NameKindArg, String> {
    let wanted = text.to_ascii_lowercase();
    if !wanted.is_empty() {
        let mut found = NameKindArg::ALL
            .iter()
            .filter(|(name, _)| name.starts_with(&wanted));
        if let (Some((_, kind)), None) = (found.next(), found.next()) {
            return Ok(*kind);
        }
    }
    Err(format!(
        "invalid kind of name '{text}': expected player, event, site or round (or a unique prefix)"
    ))
}

/// Arguments of `names`.
#[derive(Debug, Args)]
pub struct NamesArgs {
    /// The database.
    pub base: PathBuf,
    /// The kind of name: player, event, site or round (a unique prefix is enough).
    #[arg(value_parser = parse_name_kind)]
    pub kind: NameKindArg,
    /// Only names starting with this text (case sensitive).
    pub prefix: Option<String>,
}

/// Arguments of `describe`.
#[derive(Debug, Args)]
pub struct DescribeArgs {
    /// The database.
    pub base: PathBuf,
    /// The new description.
    pub text: String,
}

/// Arguments of `sort`.
#[derive(Debug, Args)]
pub struct SortArgs {
    /// Reverse the whole order.
    #[arg(short, long)]
    pub reverse: bool,
    /// Sort fields separated by commas: date, year, event, site, round,
    /// white, black, eco, result, length, rating, welo, belo, country,
    /// month, deleted, eventdate, variations, comments.
    #[arg(value_parser = parse_fields)]
    pub fields: SortFields,
    /// The database.
    pub base: PathBuf,
}

/// A validated list of sort fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortFields(pub Vec<SortKey>);

fn parse_fields(text: &str) -> Result<SortFields, SortKeyError> {
    SortKey::parse_list(text).map(SortFields)
}

/// Arguments of `compact`.
#[derive(Debug, Args)]
pub struct CompactArgs {
    /// Compact the names (remove unused ones) instead of the games.
    #[arg(short, long)]
    pub names: bool,
    /// The database.
    pub base: PathBuf,
}

/// Arguments of `export`.
#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Add the games to the end of an existing file.
    #[arg(short, long)]
    pub append: bool,
    /// The database.
    pub base: PathBuf,
    /// The PGN file (`.pgn` is added if missing).
    pub pgn: PathBuf,
}
