/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use blobstore_factory::make_packblob;
use borrowed::borrowed;
use clap_old::Arg;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream;
use futures::stream::TryStreamExt;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use std::io;
use std::io::BufRead;

mod pack_utils;

const ARG_ZSTD_LEVEL: &str = "zstd-level";
const ARG_INNER_ID: &str = "inner-blobstore-id";
const ARG_DRY_RUN: &str = "dry-run";
const ARG_SCHEDULED_MAX: &str = "scheduled-max";

const PACK_PREFIX: &str = "multiblob-";

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("Packer")
        .with_advanced_args_hidden()
        .with_scuba_logging_args()
        .with_repo_required(args::RepoRequirement::ExactlyOne)
        .build()
        .about("Given a set of blob names on stdin, replace them with a packed version that takes less space")
        .arg(
            Arg::with_name(ARG_INNER_ID)
                .long(ARG_INNER_ID)
                .takes_value(true)
                .required(false)
                .help("If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id")
        )
        .arg(
            Arg::with_name(ARG_ZSTD_LEVEL)
                .long(ARG_ZSTD_LEVEL)
                .takes_value(true)
                .required(true)
                .help("zstd compression level to use")
        )
        .arg(
            Arg::with_name(ARG_DRY_RUN)
            .long(ARG_DRY_RUN)
            .takes_value(true)
            .required(false)
            .help("If true, do not upload the finished pack to the blobstore")
        )
        .arg(
            Arg::with_name(ARG_SCHEDULED_MAX)
                .long(ARG_SCHEDULED_MAX)
                .takes_value(true)
                .required(false)
                .help("Maximum number of parallel packs to work on. Default 10"),
        )
}

fn get_blobconfig(
    mut blob_config: BlobConfig,
    inner_blobstore_id: Option<u64>,
) -> Result<BlobConfig> {
    // If the outer store is a mux, find th requested inner store
    if let Some(inner_blobstore_id) = inner_blobstore_id {
        blob_config = match blob_config {
            BlobConfig::Multiplexed { blobstores, .. } => {
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

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches(fb)?;

    let logger = matches.logger();
    let runtime = matches.runtime();
    let config_store = matches.config_store();

    let ctx = CoreContext::new_for_bulk_processing(fb, logger.clone());
    let blobstore_options = matches.blobstore_options();
    let readonly_storage = matches.readonly_storage();
    let blobconfig = args::get_config(config_store, &matches)?
        .1
        .storage_config
        .blobstore;
    let inner_id = matches
        .value_of(ARG_INNER_ID)
        .map(str::parse::<u64>)
        .transpose()?;
    let zstd_level = matches
        .value_of(ARG_ZSTD_LEVEL)
        .map(str::parse::<i32>)
        .transpose()?
        .expect("Required argument not present");
    let dry_run = matches
        .value_of(ARG_DRY_RUN)
        .map(str::parse::<bool>)
        .transpose()?
        .unwrap_or(false);

    let repo_prefix = {
        let repo_id = args::get_repo_id(config_store, &matches)?;
        repo_id.prefix()
    };

    let max_parallelism = matches
        .value_of(ARG_SCHEDULED_MAX)
        .map_or(Ok(10), str::parse::<usize>)?;

    let input_lines: Vec<String> = io::stdin()
        .lock()
        .lines()
        .collect::<Result<_, io::Error>>()?;

    let mut scuba = matches.scuba_sample_builder();
    scuba.add_opt("blobstore_id", inner_id);

    runtime.block_on(async move {
        let blobstore = make_packblob(
            fb,
            get_blobconfig(blobconfig, inner_id)?,
            *readonly_storage,
            blobstore_options,
            logger,
            config_store,
        )
        .await?;
        stream::iter(input_lines.split(String::is_empty).map(Result::Ok))
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
    })
}
