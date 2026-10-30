//! `compact` and `describe` (`scidt -C`, `-N`, `-d`).

use anyhow::{Context, Result};
use mychess_db::si4::{NameKind, Si4Writer, compact_games, compact_names};

use crate::args::{CompactArgs, DescribeArgs};
use crate::paths::strip_db_extension;
use crate::{Ctx, EXIT_FAILURE};

fn open_for_writing(base: &std::path::Path) -> Result<Si4Writer> {
    let stem = strip_db_extension(base);
    Si4Writer::open(&stem)
        .with_context(|| format!("cannot open the database {} for writing", stem.display()))
}

pub(crate) fn run(ctx: &mut Ctx<'_>, args: &CompactArgs) -> Result<u8> {
    let writer = open_for_writing(&args.base)?;
    if !ctx.quiet {
        writeln!(ctx.err, "Compacting the database...")?;
    }
    if args.names {
        let (_writer, report) = compact_names(writer).context("cannot compact the names")?;
        if report.changed {
            for (kind, removed) in NameKind::ALL.iter().zip(report.removed) {
                writeln!(ctx.out, "Removed {removed} unused {kind} names.")?;
            }
            writeln!(ctx.out, "Names were successfully compacted.")?;
        } else {
            writeln!(ctx.out, "Every name is in use: nothing to compact.")?;
        }
    } else {
        let (_writer, report) = compact_games(writer).context("cannot compact the games")?;
        writeln!(
            ctx.out,
            "Database was successfully compacted: {} games kept, {} deleted, {} corrupt \
             dropped; game file {} -> {} bytes.",
            report.kept,
            report.deleted,
            report.corrupt,
            report.games_bytes_before,
            report.games_bytes_after
        )?;
    }
    if ctx.cancel.is_cancelled() {
        writeln!(
            ctx.err,
            "interrupted: the signal arrived while compacting; the operation was completed and the database is intact"
        )?;
        return Ok(EXIT_FAILURE);
    }
    Ok(0)
}

pub(crate) fn run_describe(ctx: &mut Ctx<'_>, args: &DescribeArgs) -> Result<u8> {
    let mut writer = open_for_writing(&args.base)?;
    writer
        .set_description(&args.text)
        .context("cannot write the description")?;
    writeln!(
        ctx.out,
        "Description changed to: {}",
        mychess_db::si4::bytes_to_string(writer.header().description())
    )?;
    Ok(0)
}
