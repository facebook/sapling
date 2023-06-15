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

#[allow(dead_code)]
struct BlobstoreBulkUnlinker {
    app: MononokeApp,
    keys_dir: String,
    dry_run: bool,
}

impl BlobstoreBulkUnlinker {
    fn new(app: MononokeApp, keys_dir: String, dry_run: bool) -> BlobstoreBulkUnlinker {
        BlobstoreBulkUnlinker {
            app,
            keys_dir,
            dry_run,
        }
    }

    fn read_lines<P>(&self, file_path: P) -> io::Result<io::Lines<io::BufReader<File>>>
    where
        P: AsRef<Path>,
    {
        let file = File::open(file_path)?;
        Ok(io::BufReader::new(file).lines())
    }

    async fn unlink_the_key_from_blobstore(&self, key: &str) -> Result<()> {
        if self.dry_run {
            writeln!(std::io::stdout(), "\tUnlink key: {}", key)?;
            return Ok(());
        }
        bail!("unimplemented unlink {} from blobstore yet!", key)
    }

    async fn bulk_unlink_keys_in_file(
        &self,
        path: &PathBuf,
        cur: usize,
        total_file_count: usize,
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
            self.dry_run,
            path.display()
        )?;

        if let Ok(lines) = self.read_lines(path) {
            for line in lines {
                if let Ok(key) = line {
                    self.unlink_the_key_from_blobstore(&key).await?;
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

    async fn start_unlink(&self) -> Result<()> {
        let entries = fs::read_dir(self.keys_dir.clone())?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, io::Error>>()?;

        let total_file_count = entries.len();
        for (cur, entry) in entries.iter().enumerate() {
            self.bulk_unlink_keys_in_file(entry, cur, total_file_count)
                .await?;
        }
        Ok(())
    }
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

    let unlinker = BlobstoreBulkUnlinker::new(app, keys_dir.clone(), dry_run);
    unlinker.start_unlink().await?;

    Ok(())
}
