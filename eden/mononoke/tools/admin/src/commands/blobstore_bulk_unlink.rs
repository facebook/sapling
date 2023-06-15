/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::io;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use mononoke_app::MononokeApp;

#[derive(Parser)]
pub struct CommandArgs {
    /// The directory that contains all the key files
    #[arg(short, long)]
    keys_dir: String,

    /// If we're dry running the command, print out the blobstore keys to be deleted
    #[arg(short, long, default_value_t = true)]
    dry_run: bool,
}

async fn bulk_unlink_keys_in_file(path: PathBuf, dry_run: bool) -> Result<()> {
    writeln!(
        std::io::stdout(),
        "Processing keys in file (with dry-run={}): {}",
        dry_run,
        path.display()
    )?;
    Ok(())
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let _ctx = app.new_basic_context();

    let keys_dir = args.keys_dir;
    let dry_run = args.dry_run;

    if dry_run {
        writeln!(
            std::io::stdout(),
            "Running the bulk deletion with a dry-run mode. Please use --dry-run false to perform the real deletion."
        )?;
    }

    let entries = fs::read_dir(keys_dir)?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, io::Error>>()?;
    for entry in entries {
        bulk_unlink_keys_in_file(entry, dry_run).await?;
    }

    Ok(())
}
