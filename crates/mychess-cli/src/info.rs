//! `info` and `names`: read-only reports (`scidt -i`, `-c`, `-n`, `-p/-e/-s/-r`).

use anyhow::{Context, Result};
use mychess_db::maintenance::{compaction_stats, name_frequencies};
use mychess_db::si4::{NameKind, SUPPORTED_VERSION, Si4Database, bytes_to_string};

use crate::Ctx;
use crate::args::{InfoArgs, NameKindArg, NamesArgs};
use crate::paths::strip_db_extension;

fn open(base: &std::path::Path) -> Result<Si4Database> {
    let stem = strip_db_extension(base);
    Si4Database::open(&stem).with_context(|| format!("cannot open the database {}", stem.display()))
}

pub(crate) fn run_info(ctx: &mut Ctx<'_>, args: &InfoArgs) -> Result<u8> {
    let db = open(&args.base)?;
    let stem = strip_db_extension(&args.base);
    let names = db.names();
    let header = db.header();
    writeln!(
        ctx.out,
        "Database \"{}\": {}",
        stem.display(),
        bytes_to_string(header.description())
    )?;
    writeln!(ctx.out, "{} games", db.len())?;
    writeln!(
        ctx.out,
        "Version: {}.{}",
        SUPPORTED_VERSION / 100,
        SUPPORTED_VERSION % 100
    )?;
    writeln!(
        ctx.out,
        "Namebase: {} players, {} events, {} sites, {} rounds.",
        names.num_names(NameKind::Player),
        names.num_names(NameKind::Event),
        names.num_names(NameKind::Site),
        names.num_names(NameKind::Round),
    )?;
    if args.compact {
        write_compactness(ctx, &db)?;
    }
    if args.names {
        write_name_stats(ctx, &db)?;
    }
    Ok(0)
}

/// `printCompactInfo`: the size of the game file now and after compacting.
fn write_compactness(ctx: &mut Ctx<'_>, db: &Si4Database) -> Result<()> {
    let stats = compaction_stats(db);
    let kb = |bytes: u64| (bytes + 512) / 1024;
    writeln!(
        ctx.out,
        "Current size:   {:>7} games, {:>8} bytes = {:>5} Kb",
        stats.games,
        stats.current_bytes,
        kb(stats.current_bytes)
    )?;
    writeln!(
        ctx.out,
        "Compacted size: {:>7} games, {:>8} bytes = {:>5} Kb",
        stats.kept_games,
        stats.compacted_bytes,
        kb(stats.compacted_bytes)
    )?;
    Ok(())
}

const KINDS: [(NameKind, &str); 4] = [
    (NameKind::Player, "PLAYER"),
    (NameKind::Event, "EVENT"),
    (NameKind::Site, "SITE"),
    (NameKind::Round, "ROUND"),
];

/// `printNameInfo` (reduced): the frequencies are recounted from the index.
fn write_name_stats(ctx: &mut Ctx<'_>, db: &Si4Database) -> Result<()> {
    let names = db.names();
    let counts = name_frequencies(db).counts;
    for ((kind, label), freq) in KINDS.iter().zip(&counts) {
        let unused = freq.iter().filter(|&&f| f == 0).count();
        writeln!(
            ctx.out,
            "\n{label} section:    {} names, {unused} unused",
            names.num_names(*kind)
        )?;
        let longest = (0..names.num_names(*kind))
            .filter_map(|id| names.get(*kind, id))
            .fold(None::<&[u8]>, |best, name| match best {
                Some(b) if b.len() >= name.len() => Some(b),
                _ => Some(name),
            });
        if let Some(name) = longest {
            writeln!(
                ctx.out,
                "Longest name: \"{}\" ({} bytes)",
                bytes_to_string(name),
                name.len()
            )?;
        }
        let top = freq
            .iter()
            .enumerate()
            .fold(None::<(usize, u32)>, |best, (id, &f)| match best {
                Some((_, b)) if b >= f => best,
                _ => Some((id, f)),
            });
        if let Some((id, f)) = top
            && let Some(name) = u32::try_from(id).ok().and_then(|id| names.get(*kind, id))
        {
            writeln!(
                ctx.out,
                "Most frequent name: \"{}\" ({f} times)",
                bytes_to_string(name)
            )?;
        }
    }
    Ok(())
}

pub(crate) fn run_names(ctx: &mut Ctx<'_>, args: &NamesArgs) -> Result<u8> {
    let db = open(&args.base)?;
    let kind = match args.kind {
        NameKindArg::Player => NameKind::Player,
        NameKindArg::Event => NameKind::Event,
        NameKindArg::Site => NameKind::Site,
        NameKindArg::Round => NameKind::Round,
    };
    let names = db.names();
    // Compared on the decoded text, like the listing: it also matches names
    // stored in Latin-1/Windows-1252 by a prefix typed in UTF-8 (D634).
    let prefix = args.prefix.as_deref().unwrap_or("");
    let mut matched = 0u32;
    for id in 0..names.num_names(kind) {
        let Some(name) = names.get(kind, id) else {
            continue;
        };
        let text = bytes_to_string(name);
        if text.starts_with(prefix) {
            let frequency = names.frequency(kind, id).unwrap_or(0);
            writeln!(ctx.out, "{frequency:>6}\t{text}")?;
            matched += 1;
        }
    }
    writeln!(
        ctx.out,
        "[{matched} names matched the supplied prefix string]"
    )?;
    Ok(0)
}
