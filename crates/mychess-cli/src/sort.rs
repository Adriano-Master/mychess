//! `sort`: permanent sort of the index (`scidt -S` / `-R`).

use anyhow::{Context, Result};
use mychess_db::si4::{Si4Writer, sort_games};

use crate::args::SortArgs;
use crate::paths::strip_db_extension;
use crate::{Ctx, EXIT_FAILURE};

pub(crate) fn run(ctx: &mut Ctx<'_>, args: &SortArgs) -> Result<u8> {
    let stem = strip_db_extension(&args.base);
    let writer = Si4Writer::open(&stem)
        .with_context(|| format!("cannot open the database {} for writing", stem.display()))?;
    if !ctx.quiet {
        writeln!(ctx.err, "Sorting the database...")?;
    }
    let (_writer, report) = sort_games(writer, &args.fields.0, args.reverse)
        .with_context(|| format!("cannot sort {}", stem.display()))?;
    writeln!(
        ctx.out,
        "Database was successfully sorted ({} games, {} moved).",
        report.games, report.moved
    )?;
    if !ctx.quiet {
        writeln!(
            ctx.out,
            "Only the index file was changed, so games are now stored in a scrambled order\n\
             in the game file. This makes some searches slower: run\n    \
             mychess-cli compact {}\nto compact the database.",
            stem.display()
        )?;
    }
    if ctx.cancel.is_cancelled() {
        writeln!(
            ctx.err,
            "interrupted: the signal arrived while sorting; the sort was completed and the database is intact"
        )?;
        return Ok(EXIT_FAILURE);
    }
    Ok(0)
}
