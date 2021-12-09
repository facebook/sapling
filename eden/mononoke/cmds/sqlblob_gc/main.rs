/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::ops::Range;

use anyhow::{anyhow, bail, Context, Result};
use clap::Arg;
use fbinit::FacebookInit;

use blobstore_factory::make_sql_blobstore;
use cmdlib::args::{self, MononokeClapApp};
use metaconfig_types::{BlobConfig, BlobstoreId, ShardableRemoteDatabaseConfig};

mod subcommand_log_size;
mod subcommand_mark;

const ARG_STORAGE_CONFIG_NAME: &str = "storage-config-name";
const ARG_SCHEDULED_MAX: &str = "scheduled-max";
const ARG_START_SHARD: &str = "start-shard";
const ARG_SHARD_COUNT: &str = "shard-count";
const ARG_INNER_ID: &str = "inner-blobstore-id";

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("SQLblob GC")
        .with_scuba_logging_args()
        .with_advanced_args_hidden()
        .with_all_repos()
        .build()
        .about("Perform garbage collection on a set of SQLblob shards")
        .arg(
            Arg::with_name(ARG_STORAGE_CONFIG_NAME)
                .long(ARG_STORAGE_CONFIG_NAME)
                .takes_value(true)
                .required(true)
                .help("the name of the storage config to GC. This *must* be an XDB storage config, or a multiplex containing an XDB (in which case, give the inner blobstore ID, too"),
        )
        .arg(
            Arg::with_name(ARG_INNER_ID)
                .long(ARG_INNER_ID)
                .takes_value(true)
                .required(false)
                .help("If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id")
        )
        .arg(
            Arg::with_name(ARG_SCHEDULED_MAX)
                .long(ARG_SCHEDULED_MAX)
                .takes_value(true)
                .required(false)
                .help("Maximum number of parallel keys to GC.  Default 100."),
        )
        .arg(
            Arg::with_name(ARG_START_SHARD)
                .long(ARG_START_SHARD)
                .help("Metadata shard number to start at (or 0 if not specified")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_SHARD_COUNT)
                .long(ARG_SHARD_COUNT)
                .help("Number of shards to walk (or all shards up to the maximum shard number if not specified")
                .takes_value(true)
                .required(false),
        )
        .subcommand(subcommand_mark::build_subcommand())
        .subcommand(subcommand_log_size::build_subcommand())
}

fn remove_wrapper_blobconfigs(mut blob_config: BlobConfig) -> BlobConfig {
    // Pack is a wrapper store - remove it
    while let BlobConfig::Pack { ref blobconfig, .. } = blob_config {
        blob_config = BlobConfig::clone(blobconfig);
    }
    blob_config
}

fn get_blobconfig(blob_config: BlobConfig, inner_blobstore_id: Option<u64>) -> Result<BlobConfig> {
    let mut blob_config = remove_wrapper_blobconfigs(blob_config);

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

    Ok(remove_wrapper_blobconfigs(blob_config))
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches(fb)?;
    let matches = &matches;

    let logger = matches.logger().clone();

    let config_store = matches.config_store();

    let runtime = matches.runtime();

    let max_parallelism = matches
        .value_of(ARG_SCHEDULED_MAX)
        .map_or(Ok(100), str::parse::<usize>)?;

    let inner_blobstore_id = matches
        .value_of(ARG_INNER_ID)
        .map(str::parse::<u64>)
        .transpose()?;

    let blobstore_config = {
        let storage_config = args::load_storage_configs(config_store, &matches)
            .context("Could not read storage configs")?
            .storage
            .remove(
                matches
                    .value_of(ARG_STORAGE_CONFIG_NAME)
                    .context("No storage config name")?,
            )
            .context("Requested storage config not found")?;
        storage_config.blobstore
    };
    let blobstore_config = get_blobconfig(blobstore_config, inner_blobstore_id)?;

    let shard_range = {
        let max_shards = match &blobstore_config {
            BlobConfig::Mysql { remote } => {
                if let ShardableRemoteDatabaseConfig::Sharded(config) = remote {
                    config.shard_num.get()
                } else {
                    1
                }
            }
            _ => 1,
        };
        let shard_start = matches
            .value_of(ARG_START_SHARD)
            .map_or(Ok(0), str::parse::<usize>)?;
        let shard_count = matches
            .value_of(ARG_SHARD_COUNT)
            .map_or(Ok(max_shards), str::parse::<usize>)?;

        Range {
            start: shard_start,
            end: shard_start + shard_count,
        }
    };

    let blobstore_options = matches.blobstore_options();

    runtime.block_on(async move {
        let blobstore = make_sql_blobstore(
            fb,
            blobstore_config,
            blobstore_factory::ReadOnlyStorage(false),
            &blobstore_options,
            &config_store,
        )
        .await?
        .into_inner();

        match matches.subcommand() {
            (subcommand_mark::MARK_SAFE, Some(sub_m)) => {
                subcommand_mark::subcommand_mark(
                    fb,
                    logger,
                    sub_m,
                    max_parallelism,
                    blobstore,
                    shard_range,
                )
                .await
            }
            (subcommand_log_size::LOG_SIZE, Some(sub_m)) => {
                subcommand_log_size::subcommand_log_size(
                    logger,
                    sub_m,
                    max_parallelism,
                    blobstore,
                    shard_range,
                    matches.scuba_sample_builder(),
                )
                .await
            }
            _ => Err(anyhow!(matches.usage().to_string())),
        }
    })
}
