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
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::BlobstoreUnlinkOps;
use clap::ArgAction;
use clap::Parser;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mononoke_types::RepositoryId;
use regex::Regex;

use crate::commands::blobstore_unlink::get_blobstores;

#[derive(Parser)]
pub struct CommandArgs {
    /// The directory that contains all the key files
    #[arg(short, long)]
    keys_dir: String,

    /// If we're dry running the command, print out the blobstore keys to be deleted
    #[arg(short, long, default_value_t = true, action = ArgAction::Set)]
    dry_run: bool,

    /// If we should print out the error message when we see:
    ///  * failed to parse a key
    ///  * failed to unlink a key
    #[arg(short, long, default_value_t = false, action = ArgAction::Set)]
    verbose: bool,
}

#[allow(dead_code)]
struct BlobstoreBulkUnlinker {
    app: MononokeApp,
    keys_dir: String,
    dry_run: bool,
    verbose: bool,
}

impl BlobstoreBulkUnlinker {
    fn new(
        app: MononokeApp,
        keys_dir: String,
        dry_run: bool,
        verbose: bool,
    ) -> BlobstoreBulkUnlinker {
        BlobstoreBulkUnlinker {
            app,
            keys_dir,
            dry_run,
            verbose,
        }
    }

    fn read_lines<P>(&self, file_path: P) -> io::Result<io::Lines<io::BufReader<File>>>
    where
        P: AsRef<Path>,
    {
        let file = File::open(file_path)?;
        Ok(io::BufReader::new(file).lines())
    }

    fn extract_repo_id_from_key(&self, key: &str) -> Result<RepositoryId, Error> {
        self.check_if_key_valid(key)?;
        let re = Regex::new(r".*repo([0-9]+)..*")?;
        let caps = re
            .captures(key)
            .with_context(|| format!("Failed to capture lambda for key {}", key))?;
        let repo_id_str = caps.get(1).map_or("", |m| m.as_str());
        RepositoryId::from_str(repo_id_str)
    }

    fn extract_blobstore_key_from(&self, key: &str) -> Result<String, Error> {
        self.check_if_key_valid(key)?;
        let re = Regex::new(r".*(repo[0-9]+..*)")?;
        let caps = re
            .captures(key)
            .with_context(|| format!("Failed to capture lambda for key {}", key))?;
        let blobstore_key = caps.get(1).map_or("", |m| m.as_str());
        Ok(blobstore_key.to_string())
    }

    fn check_if_key_valid(&self, key: &str) -> Result<(), Error> {
        let re = Regex::new(r".*repo([0-9]+)..*")?;
        let _caps = re
            .captures(key)
            .with_context(|| format!("Invalid blobstore key {}", key))?;
        Ok(())
    }

    async fn get_blobstores_from_repo_id(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Vec<Arc<dyn BlobstoreUnlinkOps>>> {
        let (_repo_name, repo_config) = self.app.repo_config(&RepoArg::Id(repo_id))?;
        let blobstores = get_blobstores(
            self.app.fb,
            repo_config.storage_config,
            None,
            self.app.environment().readonly_storage,
            &self.app.environment().blobstore_options,
            self.app.config_store(),
        )
        .await?;
        Ok(blobstores)
    }

    async fn unlink_the_key_from_blobstore(&self, key: &str) -> Result<()> {
        if self.dry_run {
            writeln!(std::io::stdout(), "\tUnlink key: {}", key)?;
            return Ok(());
        }

        if let (Ok(repo_id), Ok(blobstore_key)) = (
            self.extract_repo_id_from_key(key),
            self.extract_blobstore_key_from(key),
        ) {
            let blobstores = self.get_blobstores_from_repo_id(repo_id).await?;

            let mut num_errors = 0;
            let num_blobstores = blobstores.len();
            for blobstore in blobstores {
                match blobstore
                    .unlink(&self.app.new_basic_context(), &blobstore_key)
                    .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        num_errors += 1;
                        let error_msg = e.to_string();
                        if !error_msg.contains("does not exist in the blobstore")
                            && !error_msg.contains("[404] Path not found")
                        {
                            if self.verbose.clone() {
                                writeln!(
                                    std::io::stdout(),
                                    "Failed to unlink key {} in one underlying blobstore, error: {}.",
                                    blobstore_key,
                                    error_msg
                                )?;
                            }
                        }
                    }
                }
            }

            if num_errors == num_blobstores {
                if self.verbose.clone() {
                    writeln!(
                        std::io::stdout(),
                        "For key {}, no blobstore contained this key.",
                        blobstore_key,
                    )?;
                }
            }
        } else {
            if self.verbose.clone() {
                writeln!(std::io::stdout(), "\tSkip invalid key: {}", key)?;
            }
            return Ok(());
        }

        Ok(())
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
    let keys_dir = args.keys_dir;
    let dry_run = args.dry_run;
    let verbose = args.verbose;

    if dry_run {
        writeln!(
            std::io::stdout(),
            "Running the bulk deletion with a dry-run mode. Please use --dry-run false to perform the real deletion."
        )?;
    }

    let unlinker = BlobstoreBulkUnlinker::new(app, keys_dir.clone(), dry_run, verbose);
    unlinker.start_unlink().await?;

    Ok(())
}
