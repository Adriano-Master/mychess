//! `merge`: several databases into a new one (`scmerge`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mychess_db::si4::{GameFlags, PreparedGame, Si4Database, Si4Writer};

use crate::args::MergeArgs;
use crate::paths::{database_exists, find_existing, remove_database, strip_db_extension};
use crate::{Ctx, EXIT_FAILURE};

/// Games encoded and written at a time.
const BATCH: usize = 500;

pub(crate) fn run(ctx: &mut Ctx<'_>, args: &MergeArgs) -> Result<u8> {
    let target = strip_db_extension(&args.target);
    if database_exists(&target) {
        bail!("the database {} already exists", target.display());
    }
    let sources = collect_sources(ctx, &args.sources)?;
    if sources.is_empty() {
        bail!("no database to merge");
    }
    for source in &sources {
        if find_existing(source, "si4").is_none() {
            bail!("cannot open {}: no .si4 file", source.display());
        }
    }
    let mut writer = Si4Writer::create(&target, 0)
        .with_context(|| format!("cannot create the database {}", target.display()))?;
    let outcome = merge_all(ctx, &mut writer, &sources);
    let games = writer.len();
    drop(writer);
    match outcome {
        Ok(Merged::Done { skipped }) => {
            if skipped > 0 {
                writeln!(
                    ctx.out,
                    "Successfully merged {} files, of {games} games ({skipped} skipped).",
                    sources.len()
                )?;
                writeln!(ctx.err, "error: {skipped} games could not be copied")?;
                return Ok(EXIT_FAILURE);
            }
            writeln!(
                ctx.out,
                "Successfully merged {} files, of {games} games.",
                sources.len()
            )?;
            Ok(0)
        }
        Ok(Merged::Cancelled) => {
            if discard_partial(ctx, &target)? {
                writeln!(ctx.err, "interrupted: {} was not created", target.display())?;
            } else {
                writeln!(ctx.err, "interrupted: the merge was stopped")?;
            }
            Ok(EXIT_FAILURE)
        }
        Err(error) => {
            discard_partial(ctx, &target)?;
            Err(error)
        }
    }
}

/// Removes the partial database after a failed or cancelled merge. A failure
/// is reported as a warning, never hidden; returns whether it is gone.
fn discard_partial(ctx: &mut Ctx<'_>, target: &Path) -> Result<bool> {
    match remove_database(target) {
        Ok(()) => Ok(true),
        Err(error) => {
            writeln!(
                ctx.err,
                "warning: could not remove the partial database {}: {error}",
                target.display()
            )?;
            Ok(false)
        }
    }
}

enum Merged {
    Done { skipped: u64 },
    Cancelled,
}

/// The sources, with `-` replaced by the names read from standard input.
fn collect_sources(ctx: &mut Ctx<'_>, given: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut sources = Vec::new();
    for path in given {
        if path.as_os_str() == "-" {
            let mut line = String::new();
            loop {
                line.clear();
                let read = ctx
                    .input
                    .read_line(&mut line)
                    .context("cannot read the list of databases from standard input")?;
                if read == 0 {
                    break;
                }
                let name = line.trim_end_matches(['\n', '\r']);
                if !name.is_empty() {
                    sources.push(strip_db_extension(Path::new(name)));
                }
            }
        } else {
            sources.push(strip_db_extension(path));
        }
    }
    Ok(sources)
}

fn merge_all(ctx: &mut Ctx<'_>, writer: &mut Si4Writer, sources: &[PathBuf]) -> Result<Merged> {
    let mut skipped = 0u64;
    for source in sources {
        let mut db = Si4Database::open(source)
            .with_context(|| format!("cannot open {}", source.display()))?;
        if !ctx.quiet {
            writeln!(ctx.err, "merge: {} ({} games)", source.display(), db.len())?;
        }
        let mut batch: Vec<PreparedGame> = Vec::with_capacity(BATCH);
        for n in 1..=db.len() {
            if ctx.cancel.is_cancelled() {
                return Ok(Merged::Cancelled);
            }
            let prepared = db.entry(n).map_err(anyhow::Error::from).and_then(|entry| {
                let game = db.game(n)?;
                Ok(PreparedGame::from_game(
                    &game,
                    entry.flags & GameFlags::EDITABLE,
                )?)
            });
            match prepared {
                Ok(game) => batch.push(game),
                Err(error) => {
                    skipped += 1;
                    writeln!(
                        ctx.err,
                        "warning: {}: game {n} skipped: {error:#}",
                        source.display()
                    )?;
                }
            }
            if batch.len() >= BATCH {
                writer
                    .add_prepared_batch(&batch)
                    .context("cannot write the new database")?;
                batch.clear();
            }
        }
        writer
            .add_prepared_batch(&batch)
            .context("cannot write the new database")?;
    }
    Ok(Merged::Done { skipped })
}
