/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::fs::metadata;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::bail;
use anyhow::Result;
use clap::ArgAction;
use clap::Parser;
use mononoke_app::MononokeApp;

#[derive(Parser)]
pub struct CommandArgs {
    /// The directory that contains all the key files
    #[arg(short, long)]
    keys_dir: String,

    /// If we're dry running the command, print out the blobstore keys to be deleted
    #[arg(short, long, default_value_t = true, action = ArgAction::Set)]
    dry_run: bool,
}

fn read_lines<P>(file_path: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(file_path)?;
    Ok(io::BufReader::new(file).lines())
}

async fn unlink_the_key_from_blobstore(key: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        writeln!(std::io::stdout(), "\tUnlink key: {}", key);
        return Ok(());
    }
    bail!("unimplemented unlink {} from blobstore yet!", key)
}

async fn bulk_unlink_keys_in_file(
    path: &PathBuf,
    cur: usize,
    total_file_count: usize,
    dry_run: bool,
) -> Result<()> {
    let md = metadata(path.clone()).unwrap();
    if !md.is_file() {
        writeln!(
            std::io::stdout(),
            "Skip path: {} because it is not a file.",
            path.display(),
        )?;
        return Ok(());
    }

    writeln!(
        std::io::stdout(),
        "Processing keys in file (with dry-run={}): {}",
        dry_run,
        path.display()
    )?;

    if let Ok(lines) = read_lines(path) {
        for line in lines {
            if let Ok(key) = line {
                unlink_the_key_from_blobstore(&key, dry_run).await?;
            }
        }
        writeln!(
            std::io::stdout(),
            "Progress: {:.3}%",
            (cur + 1) as f32 * 100.0 / total_file_count as f32
        )?;
    }

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

    let total_file_count = entries.len();
    for (cur, entry) in entries.iter().enumerate() {
        bulk_unlink_keys_in_file(entry, cur, total_file_count, dry_run).await?;
    }

    Ok(())
}
