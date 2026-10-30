//! `import`: a PGN file into a new SI4 database (`pgnscid`).

use std::fs::{self, File};

use anyhow::{Context, Result, bail};
use mychess_db::si4::{NameKind, Si4Writer};
use mychess_db::{ImportOptions, import_pgn_file};
use mychess_pgn::read::PreGameText;

use crate::args::ImportArgs;
use crate::paths::{
    case_variants, database_files, default_base, find_existing, remove_files, strip_db_extension,
    with_suffix,
};
use crate::progress::Reporter;
use crate::{Ctx, EXIT_FAILURE};

pub(crate) fn run(ctx: &mut Ctx<'_>, args: &ImportArgs) -> Result<u8> {
    // Open the PGN first: a missing file must not leave a database behind.
    File::open(&args.pgn).with_context(|| format!("cannot open {}", args.pgn.display()))?;
    let stem = args
        .base
        .as_deref()
        .map_or_else(|| default_base(&args.pgn), strip_db_extension);
    let existing = find_existing(&stem, "si4").and_then(|p| fs::metadata(p).ok());
    if existing.is_some_and(|m| m.len() > 0) && !args.force {
        bail!(
            "database already exists: {} (use --force to overwrite it)",
            stem.display()
        );
    }
    if args.force {
        // Say what goes away before it does (D631); also with --quiet.
        let old = database_files(&stem);
        // The writer refuses a name that differs only in case from another
        // file (they are one file on Windows and macOS): fail before deleting.
        if let Some(clash) = case_variants(&stem, &old).first() {
            bail!(
                "{} differs from the database {} only in letter case; remove or rename it first (nothing was removed)",
                clash.display(),
                stem.display()
            );
        }
        for path in &old {
            writeln!(ctx.err, "warning: --force removes {}", path.display())?;
        }
        remove_files(&old)
            .with_context(|| format!("cannot remove the old database {}", stem.display()))?;
    }
    let mut writer = Si4Writer::create(&stem, 0)
        .with_context(|| format!("cannot create the database {}", stem.display()))?;
    let log_path = with_suffix(&stem, ".err");
    if !ctx.quiet {
        writeln!(
            ctx.out,
            "Converting file {} to database {}:",
            args.pgn.display(),
            stem.display()
        )?;
        writeln!(
            ctx.out,
            "Errors/warnings will be written to {}.\n",
            log_path.display()
        )?;
    }

    let mut options = ImportOptions::cli();
    if args.ignore_pregame {
        options.pre_game_text = PreGameText::Ignore;
    }
    let mut reporter = Reporter::new(ctx.err, "import", ctx.quiet);
    let report = import_pgn_file(
        &args.pgn,
        &mut writer,
        &options,
        None,
        &mut reporter,
        ctx.cancel,
    )
    .with_context(|| format!("import into {} failed", stem.display()))?;

    let text = report.to_string();
    let messages = text.lines().count();
    if report.has_warnings() {
        fs::write(&log_path, &text)
            .with_context(|| format!("cannot write the log {}", log_path.display()))?;
    } else if let Some(old) = find_existing(&stem, "err") {
        fs::remove_file(old).ok();
    }

    let names = writer.names();
    writeln!(
        ctx.out,
        "Database '{}': {} games, {} players, {} events, {} sites.",
        stem.display(),
        writer.len(),
        names.num_names(NameKind::Player),
        names.num_names(NameKind::Event),
        names.num_names(NameKind::Site),
    )?;
    if report.has_warnings() {
        writeln!(
            ctx.out,
            "There were {messages} errors or warnings; examine the file \"{}\"",
            log_path.display()
        )?;
    } else {
        writeln!(ctx.out, "There were no warnings or errors.")?;
    }
    if report.interrupted {
        writeln!(
            ctx.err,
            "interrupted: {} games were stored before the stop",
            report.games_added
        )?;
        return Ok(EXIT_FAILURE);
    }
    if let Some(stop) = &report.stopped {
        writeln!(ctx.err, "error: {stop}")?;
        return Ok(EXIT_FAILURE);
    }
    Ok(0)
}
