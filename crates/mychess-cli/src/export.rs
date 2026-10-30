//! `export`: the games of a database as PGN.

use anyhow::{Context, Result};
use mychess_db::si4::Si4Database;
use mychess_db::{ExportItem, ExportOptions, export_pgn};

use crate::args::ExportArgs;
use crate::paths::strip_db_extension;
use crate::progress::Reporter;
use crate::{Ctx, EXIT_FAILURE};

pub(crate) fn run(ctx: &mut Ctx<'_>, args: &ExportArgs) -> Result<u8> {
    let stem = strip_db_extension(&args.base);
    let mut db = Si4Database::open(&stem)
        .with_context(|| format!("cannot open the database {}", stem.display()))?;
    let mut options = ExportOptions::scid_dialog();
    options.append = args.append;
    options.total = Some(u64::from(db.len()));
    // Lazy: each game is decoded when the exporter asks for it, so the whole
    // database is never in memory at once (D628).
    let len = db.len();
    let items = (1..=len).map(|n| ExportItem::stored(&mut db, n));
    let mut reporter = Reporter::new(ctx.err, "export", ctx.quiet);
    let report = export_pgn(items, &args.pgn, &options, &mut reporter, ctx.cancel)
        .with_context(|| format!("cannot export to {}", args.pgn.display()))?;
    for (number, reason) in &report.skipped {
        writeln!(ctx.err, "warning: game {number} skipped: {reason:?}")?;
    }
    writeln!(
        ctx.out,
        "Exported {} games to {}.",
        report.written,
        report.path.display()
    )?;
    if report.interrupted {
        writeln!(ctx.err, "interrupted: the export is incomplete")?;
        return Ok(EXIT_FAILURE);
    }
    Ok(u8::from(!report.skipped.is_empty()))
}
