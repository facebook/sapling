/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::fs::metadata;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::BufRead;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::bail;
use anyhow::format_err;
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
use serde_json::json;

use crate::commands::blobstore_unlink::get_blobconfig;

/// Unlink large numbers of blobstore keys
#[derive(Parser)]
pub struct CommandArgs {
    /// The directory that contains all the key files
    #[arg(short, long)]
    keys_dir: String,

    /// If we're dry running the command, print out the blobstore keys to be deleted
    #[arg(short, long, default_value_t = true, action = ArgAction::Set)]
    dry_run: bool,

    /// regex that is used to check if the key is suppose to be deleted
    #[arg(short, long)]
    sanitise_regex: String,

    /// The file, we're going to log the error into
    #[arg(short, long)]
    error_log_file: String,

    /// The file, we're going to track which key file has been processed
    #[arg(short, long)]
    progress_track_file: String,
}

fn create_or_open_file(error_log_file_path: String, read_before_write: bool) -> File {
    let file = OpenOptions::new()
        .create(true)
        .read(read_before_write)
        .write(true)
        .append(true)
        .open(error_log_file_path)
        .unwrap();
    file
}

#[allow(dead_code)]
struct BlobstoreBulkUnlinker {
    app: MononokeApp,
    keys_dir: String,
    dry_run: bool,
    sanitise_regex: String,
    repo_to_blobstore: HashMap<RepositoryId, Arc<dyn BlobstoreUnlinkOps>>,
    error_log_file: File,
    progress_track_file: File,
    already_processed_files: HashSet<String>,
}

impl BlobstoreBulkUnlinker {
    fn new(
        app: MononokeApp,
        keys_dir: String,
        dry_run: bool,
        sanitise_regex: String,
        error_log_file_path: String,
        progress_track_path: String,
    ) -> BlobstoreBulkUnlinker {
        BlobstoreBulkUnlinker {
            app,
            keys_dir,
            dry_run,
            sanitise_regex,
            repo_to_blobstore: HashMap::new(),
            error_log_file: create_or_open_file(error_log_file_path, false),
            progress_track_file: create_or_open_file(progress_track_path, true),
            already_processed_files: HashSet::new(),
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
        let re = Regex::new(r".*repo([0-9]+)..*")?;
        let caps = re
            .captures(key)
            .with_context(|| format!("Failed to capture lambda for key {}", key))?;
        let repo_id_str = caps.get(1).map_or("", |m| m.as_str());
        RepositoryId::from_str(repo_id_str)
    }

    fn extract_blobstore_key_from(&self, key: &str) -> Result<String, Error> {
        let re = Regex::new(r".*(repo[0-9]+..*)")?;
        let caps = re
            .captures(key)
            .with_context(|| format!("Failed to capture lambda for key {}", key))?;
        let blobstore_key = caps.get(1).map_or("", |m| m.as_str());
        Ok(blobstore_key.to_string())
    }

    async fn get_blobstore_from_repo_id(
        &mut self,
        repo_id: RepositoryId,
    ) -> Result<&dyn BlobstoreUnlinkOps> {
        use std::collections::hash_map::Entry::Vacant;
        if let Vacant(e) = self.repo_to_blobstore.entry(repo_id) {
            let (_repo_name, repo_config) = self.app.repo_config(&RepoArg::Id(repo_id))?;
            let blob_config = get_blobconfig(repo_config.storage_config.blobstore, None)?;
            let blobstore = self
                .app
                .open_blobstore_unlink_ops_with_overriden_blob_config(&blob_config)
                .await?;
            e.insert(blobstore);
        }
        return Ok(self.repo_to_blobstore.get(&repo_id).unwrap());
    }

    fn sanitise_check(&self, key: &str) -> Result<()> {
        let re = Regex::new(&self.sanitise_regex).unwrap();
        if !re.is_match(key) {
            bail!(
                "Key {} does not match the sanitise checking regex {}",
                key,
                &self.sanitise_regex
            );
        }
        Ok(())
    }

    async fn unlink_the_key_from_blobstore(&mut self, key: &str) -> Result<()> {
        let context = self.app.new_basic_context().clone();

        if let (Ok(repo_id), Ok(blobstore_key)) = (
            self.extract_repo_id_from_key(key),
            self.extract_blobstore_key_from(key),
        ) {
            // do a sanitising check before we start deleting
            self.sanitise_check(&blobstore_key)?;

            if self.dry_run {
                println!("\tUnlink key: {}", key);
                return Ok(());
            }

            if let Ok(blobstore) = self.get_blobstore_from_repo_id(repo_id).await {
                // Note that the implementation of unlink on a multiplexed blobstore won't fail if
                // the key is already absent.
                let result = blobstore.unlink(&context, &blobstore_key).await;
                if let Err(err) = result {
                    let error_msg = err.to_string();
                    if error_msg.contains("does not exist in the blobstore")
                        || error_msg.contains("[404] Path not found")
                    {
                        // If the blobstore is not multiplexed, unlink can fail because the key is
                        // not already present.
                        // If that's the case, we don't want to fail as we're already in the
                        // desired state.
                        // Instead, log the error to a file and continue.
                        self.log_error_to_file(key, "no blobstore contains this key.")
                            .await?;
                    } else {
                        bail!(err.context(format!("Failed to unlink key {}", blobstore_key)));
                    }
                }
            } else {
                // We log this error into a file. so that we can tackle them later together.
                self.log_error_to_file(
                    key,
                    "Skip key because its repo id is not found in the given repo config.",
                )
                .await?;
            }
        } else {
            self.log_error_to_file(key, "Skip key because it is invalid.")
                .await?;
        }

        Ok(())
    }

    async fn log_error_to_file(&self, key: &str, msg: &str) -> Result<()> {
        let error_record = json!({
            "key": key,
            "message": msg
        });
        writeln!(&self.error_log_file, "{}", error_record)
            .with_context(|| format_err!("Error while writing to file {:?}", self.error_log_file))
    }

    async fn log_processed_file(&self, path: &str) -> Result<()> {
        writeln!(&self.progress_track_file, "{}", path).with_context(|| {
            format_err!(
                "Error while recording progress to file {:?}",
                self.progress_track_file
            )
        })
    }

    async fn bulk_unlink_keys_in_file(
        &mut self,
        path: &PathBuf,
        cur: usize,
        total_file_count: usize,
    ) -> Result<()> {
        let md = metadata(path.clone()).unwrap();
        if !md.is_file() {
            println!("Skip path: {} because it is not a file.", path.display(),);
            return Ok(());
        }

        println!(
            "Processing keys in file (with dry-run={}): {}",
            self.dry_run,
            path.display()
        );

        if let Ok(lines) = self.read_lines(path) {
            let now = Instant::now();
            for line in lines {
                if let Ok(key) = line {
                    self.unlink_the_key_from_blobstore(&key).await?;
                }
            }
            let elapsed = now.elapsed();
            println!(
                "Progress: {:.3}%\tprocessing took {:.2?}",
                (cur + 1) as f32 * 100.0 / total_file_count as f32,
                elapsed
            );
        }

        Ok(())
    }

    async fn start_unlink(&mut self) -> Result<()> {
        let lines = io::BufReader::new(&self.progress_track_file).lines();
        for line in lines {
            if let Ok(key_file_path) = line {
                self.already_processed_files
                    .insert(key_file_path.to_string());
            }
        }

        let entries = fs::read_dir(self.keys_dir.clone())?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, io::Error>>()?;

        let total_file_count = entries.len();
        for (cur, entry) in entries.iter().enumerate() {
            let path_to_record = entry.display().to_string();
            if self.already_processed_files.contains(&path_to_record) {
                println!("File {} has already been processed, skip.", entry.display());
                println!(
                    "Progress: {:.3}%\tprocessing took 0 seconds.",
                    (cur + 1) as f32 * 100.0 / total_file_count as f32
                );
                continue;
            }
            self.bulk_unlink_keys_in_file(entry, cur, total_file_count)
                .await?;
            self.log_processed_file(&path_to_record).await?;
        }
        Ok(())
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let keys_dir = args.keys_dir;
    let dry_run = args.dry_run;
    let sanitise_regex = args.sanitise_regex;
    let error_log_file = args.error_log_file;
    let progress_track_file = args.progress_track_file;

    if dry_run {
        println!(
            "Running the bulk deletion with a dry-run mode. Please use --dry-run false to perform the real deletion."
        );
    }

    let mut unlinker = BlobstoreBulkUnlinker::new(
        app,
        keys_dir.clone(),
        dry_run,
        sanitise_regex,
        error_log_file,
        progress_track_file,
    );
    unlinker.start_unlink().await?;

    Ok(())
}
