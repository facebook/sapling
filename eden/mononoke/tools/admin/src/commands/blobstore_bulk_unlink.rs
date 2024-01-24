/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs;
use std::fs::metadata;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::BlobstoreUnlinkOps;
use clap::ArgAction;
use clap::Parser;
use cloned::cloned;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mononoke_types::RepositoryId;
use regex::Regex;

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

    /// Maximum number of parallel operations
    #[clap(
        long,
        default_value_t = 100,
        help = "Maximum number of parallel packs to work on. Default 100"
    )]
    scheduled_max: usize,
}

fn extract_repo_id_from_key(key: &str) -> Result<RepositoryId, Error> {
    let re = Regex::new(r".*repo([0-9]+)..*")?;
    let caps = re
        .captures(key)
        .with_context(|| format!("Failed to capture lambda for key {}", key))?;
    let repo_id_str = caps.get(1).map_or("", |m| m.as_str());
    RepositoryId::from_str(repo_id_str)
}

fn extract_blobstore_key_from(key: &str) -> Result<String, Error> {
    let re = Regex::new(r".*(repo[0-9]+..*)")?;
    let caps = re
        .captures(key)
        .with_context(|| format!("Failed to capture lambda for key {}", key))?;
    let blobstore_key = caps.get(1).map_or("", |m| m.as_str());
    Ok(blobstore_key.to_string())
}

fn sanitise_check(key: &str, sanitise_regex: &str) -> Result<()> {
    let re = Regex::new(sanitise_regex).unwrap();
    if !re.is_match(key) {
        bail!(
            "Key {} does not match the sanitise checking regex {}",
            key,
            sanitise_regex
        );
    }
    Ok(())
}

fn lines_from_file(filename: impl AsRef<Path>) -> Vec<String> {
    let file = File::open(filename).expect("File does not exist");
    let buf = BufReader::new(file);
    buf.lines()
        .map(|l| l.expect("Could not parse line"))
        .collect()
}

fn log_errors(key: &str, msg: &str) {
    println!("key: {}, error message: {}", key, msg)
}

async fn unlink_the_key_from_blobstore(
    key: &str,
    context: CoreContext,
    sanitise_regex: &str,
    dry_run: bool,
    repo_to_blobstore: &HashMap<RepositoryId, Arc<dyn BlobstoreUnlinkOps>>,
) -> Result<()> {
    if let (Ok(repo_id), Ok(blobstore_key)) = (
        extract_repo_id_from_key(key),
        extract_blobstore_key_from(key),
    ) {
        // do a sanitising check before we start deleting
        sanitise_check(&blobstore_key, sanitise_regex)?;

        if dry_run {
            println!("\tUnlink key: {}", key);
            return Ok(());
        }

        if !repo_to_blobstore.contains_key(&repo_id) {
            // we already logged this when building the blobstore
            return Ok(());
        }

        let blobstore = repo_to_blobstore.get(&repo_id).unwrap();
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
                log_errors(key, "no blobstore contains this key.");
            } else {
                bail!(err.context(format!("Failed to unlink key {}", blobstore_key)));
            }
        }
    } else {
        // skip this key, we have already logged this during building the blobstore for this key
    }

    Ok(())
}

#[allow(dead_code)]
struct BlobstoreBulkUnlinker {
    app: MononokeApp,
    keys_dir: String,
    dry_run: bool,
    sanitise_regex: String,
    repo_to_blobstore: HashMap<RepositoryId, Arc<dyn BlobstoreUnlinkOps>>,
    max_parallelism: usize,
}

impl BlobstoreBulkUnlinker {
    fn new(
        app: MononokeApp,
        keys_dir: String,
        dry_run: bool,
        sanitise_regex: String,
        max_parallelism: usize,
    ) -> BlobstoreBulkUnlinker {
        BlobstoreBulkUnlinker {
            app,
            keys_dir,
            dry_run,
            sanitise_regex,
            repo_to_blobstore: HashMap::new(),
            max_parallelism,
        }
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

        let now = Instant::now();
        let keys_list = lines_from_file(path);

        for key in &keys_list {
            if let Err(e) = self.add_new_blobstores(key).await {
                log_errors(key, format!("{}", e).as_str());
            }
        }

        let context = self.app.new_basic_context().clone();
        let regex = self.sanitise_regex.clone();
        let dry_run = self.dry_run;
        let repo_to_blobstore = &self.repo_to_blobstore;

        stream::iter(keys_list)
            .map(Ok)
            .try_for_each_concurrent(self.max_parallelism, |t| {
                cloned!(context, regex);
                async move {
                    let key = t;
                    unlink_the_key_from_blobstore(
                        &key,
                        context,
                        &regex.clone(),
                        dry_run,
                        repo_to_blobstore,
                    )
                    .await
                }
            })
            .await
            .with_context(|| "while unlinking keys")
            .unwrap();

        let elapsed = now.elapsed();
        println!(
            "Progress: {:.3}%\tprocessing took {:.2?}",
            (cur + 1) as f32 * 100.0 / total_file_count as f32,
            elapsed
        );
        Ok(())
    }

    async fn start_unlink(&mut self) -> Result<()> {
        let mut entries = fs::read_dir(self.keys_dir.clone())?
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, io::Error>>()?;
        entries.sort();

        let total_file_count = entries.len();
        for (cur, entry) in entries.iter().enumerate() {
            self.bulk_unlink_keys_in_file(entry, cur, total_file_count)
                .await?;
        }
        Ok(())
    }

    async fn add_new_blobstores(&mut self, key: &str) -> Result<()> {
        if let Ok(repo_id) = extract_repo_id_from_key(key) {
            let (_repo_name, repo_config) = self.app.repo_config(&RepoArg::Id(repo_id))?;
            use std::collections::hash_map::Entry::Vacant;
            if let Vacant(e) = self.repo_to_blobstore.entry(repo_id) {
                let blob_config = get_blobconfig(repo_config.storage_config.blobstore, None)?;
                if let Ok(blobstore) = self
                    .app
                    .open_blobstore_unlink_ops_with_overriden_blob_config(&blob_config)
                    .await
                {
                    e.insert(blobstore);
                } else {
                    log_errors(
                        key,
                        "Skip key because its repo id is not found in the given repo config.",
                    );
                    return Ok(());
                }
            }
        } else {
            log_errors(key, "Skip key because it is invalid.");
        }
        Ok(())
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let keys_dir = args.keys_dir;
    let dry_run = args.dry_run;
    let sanitise_regex = args.sanitise_regex;
    let scheduled_max = args.scheduled_max;

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
        scheduled_max,
    );
    unlinker.start_unlink().await?;

    Ok(())
}
