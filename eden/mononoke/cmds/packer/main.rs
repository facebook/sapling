/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use blobstore_factory::make_packblob;
use borrowed::borrowed;
use clap::Parser;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream;
use futures::stream::TryStreamExt;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use regex::Regex;

mod pack_utils;

#[derive(Parser)]
#[clap(
    about = "Given a set of blob names on stdin, replace them with a packed version that takes less space"
)]
struct MononokePackerArgs {
    #[clap(long, help = "zstd compression level to use")]
    zstd_level: i32,

    #[clap(
        long,
        help = "If true, do not upload the finished pack to the blobstore"
    )]
    dry_run: bool,

    #[clap(
        long,
        default_value_t = 1,
        help = "Maximum number of parallel packs to work on. Default 1"
    )]
    scheduled_max: usize,

    /// The directory that contains all the key files
    #[arg(short, long)]
    keys_dir: String,
}

const PACK_PREFIX: &str = "multiblob-";

fn get_blobconfig(
    mut blob_config: BlobConfig,
    inner_blobstore_id: Option<u64>,
) -> Result<BlobConfig> {
    // If the outer store is a mux, find th requested inner store
    if let Some(inner_blobstore_id) = inner_blobstore_id {
        blob_config = match blob_config {
            BlobConfig::MultiplexedWal { blobstores, .. } => {
                let required_id = BlobstoreId::new(inner_blobstore_id);
                blobstores
                    .into_iter()
                    .find_map(|(blobstore_id, _, blobstore)| {
                        if blobstore_id == required_id {
                            Some(blobstore)
                        } else {
                            None
                        }
                    })
                    .with_context(|| {
                        format!("could not find a blobstore with id {}", inner_blobstore_id)
                    })?
            }
            _ => bail!("inner-blobstore-id can only be supplied for multiplexed blobstores"),
        }
    };

    Ok(blob_config)
}

fn extract_repo_name_from_filename(filename: &str) -> &str {
    let re = Regex::new(r"repo(.*)\.store(\d*).part([0-9]+).keys.txt").unwrap();
    let caps = re
        .captures(filename)
        .with_context(|| format!("Failed to capture lambda for filename {}", filename))
        .unwrap();
    let repo_name = caps.get(1).map_or("", |m| m.as_str());
    repo_name
}

fn extract_inner_store_id_from_filename(filename: &str) -> Option<u64> {
    let re = Regex::new(r"repo(.*)\.store(\d*).part([0-9]+).keys.txt").unwrap();
    let caps = re
        .captures(filename)
        .with_context(|| format!("Failed to capture lambda for filename {}", filename))
        .unwrap();
    let inner_blobstore_id_str = caps.get(2).map_or("", |m| m.as_str());
    inner_blobstore_id_str.parse::<u64>().ok()
}

fn lines_from_file(filename: impl AsRef<Path>) -> Vec<String> {
    let file = File::open(filename).expect("File does not exist");
    let buf = BufReader::new(file);
    buf.lines()
        .map(|l| l.expect("Could not parse line"))
        .collect()
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app: MononokeApp = MononokeAppBuilder::new(fb)
        .with_app_extension(Fb303AppExtension {})
        .build::<MononokePackerArgs>()?;

    let args: MononokePackerArgs = app.args()?;
    let zstd_level = args.zstd_level;
    let dry_run = args.dry_run;
    let max_parallelism = args.scheduled_max;
    let keys_dir = args.keys_dir;

    let env = app.environment();
    let logger = app.logger();
    let runtime = app.runtime();
    let config_store = app.config_store();

    let ctx = CoreContext::new_for_bulk_processing(fb, logger.clone());
    let readonly_storage = &env.readonly_storage;
    let blobstore_options = &env.blobstore_options;

    let keys_file_entries = fs::read_dir(keys_dir)?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, io::Error>>()?;

    for (_cur, entry) in keys_file_entries.iter().enumerate() {
        let filename = entry
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("name of key file must be valid UTF-8"))?;
        // Parse repo name, and inner blobstore id from file name
        let repo_name = extract_repo_name_from_filename(filename);
        let inner_blobstore_id = extract_inner_store_id_from_filename(filename);
        // construct blobstore specific parameters
        let repo_arg = mononoke_app::args::RepoArg::Name(String::from(repo_name));
        let (_repo_name, repo_config) = app.repo_config(&repo_arg)?;
        let blobconfig = repo_config.storage_config.blobstore;
        let inner_blobconfig = get_blobconfig(blobconfig, inner_blobstore_id)?;
        let repo_prefix = repo_config.repoid.prefix();
        let mut scuba = env.scuba_sample_builder.clone();
        scuba.add_opt("blobstore_id", Some(inner_blobstore_id));
        // Read keys from the file
        let keys_list = lines_from_file(entry);
        runtime.block_on(async {
            // construct blobstore instance
            let blobstore = make_packblob(
                fb,
                inner_blobconfig,
                *readonly_storage,
                blobstore_options,
                logger,
                config_store,
            )
            .await
            .unwrap();
            // start packing
            stream::iter(keys_list.split(String::is_empty).map(Result::Ok))
                .try_for_each_concurrent(max_parallelism, |pack_keys| {
                    borrowed!(ctx, repo_prefix, blobstore, scuba);
                    async move {
                        let pack_keys: Vec<&str> = pack_keys.iter().map(|i| i.as_ref()).collect();
                        pack_utils::repack_keys(
                            ctx,
                            blobstore,
                            PACK_PREFIX,
                            zstd_level,
                            repo_prefix,
                            &pack_keys,
                            dry_run,
                            scuba,
                        )
                        .await
                    }
                })
                .await
                .with_context(|| "while packing keys")
                .unwrap();
        });
    }
    Ok(())
}

#[test]
fn test_parsing_repo_from_filename() -> Result<()> {
    let mut filename = "repoadmin.store3.part1.keys.txt";
    let mut repo_name = extract_repo_name_from_filename(filename);
    assert_eq!(repo_name, "admin");
    filename = "reporepo-hg-nolfs.store3.part1.keys.txt";
    repo_name = extract_repo_name_from_filename(filename);
    assert_eq!(repo_name, "repo-hg-nolfs");
    Ok(())
}

#[test]
fn test_parsing_inner_blobstore_id_from_filename() -> Result<()> {
    let mut filename = "repoadmin.store3.part1.keys.txt";
    let mut blobstore_id = extract_inner_store_id_from_filename(filename);
    assert_eq!(blobstore_id, Some(3));
    filename = "repoadmin.store.part1.keys.txt";
    blobstore_id = extract_inner_store_id_from_filename(filename);
    assert_eq!(blobstore_id, None);
    Ok(())
}
