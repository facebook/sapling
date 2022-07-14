/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use fbinit::FacebookInit;

use blobstore::BlobstoreUnlinkOps;
use blobstore_factory::make_sql_blobstore;
use blobstore_factory::BlobstoreOptions;
use blobstore_factory::ReadOnlyStorage;
use cached_config::ConfigStore;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use metaconfig_types::StorageConfig;
use slog::info;
use slog::Logger;

use crate::error::SubcommandError;

pub const BLOBSTORE_UNLINK: &str = "blobstore-unlink";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(BLOBSTORE_UNLINK)
        .about("unlinks blobs in the blobstore")
        .args_from_usage("[KEY]    'key of the blob to be unlinked'")
        .arg(
            Arg::with_name("inner-blobstore-id")
                .long("inner-blobstore-id")
                .takes_value(true)
                .required(false)
                .help("If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id")
        )
}

fn remove_wrapper_blobconfigs(mut blob_config: BlobConfig) -> BlobConfig {
    // Pack is a wrapper store - remove it
    while let BlobConfig::Pack { ref blobconfig, .. } = blob_config {
        blob_config = BlobConfig::clone(blobconfig);
    }
    blob_config
}

fn get_blobconfig(blob_config: BlobConfig, inner_blobstore_id: Option<u64>) -> Result<BlobConfig> {
    match inner_blobstore_id {
        None => Ok(blob_config),
        Some(inner_blobstore_id) => match blob_config {
            BlobConfig::Multiplexed { blobstores, .. } => {
                let seeked_id = BlobstoreId::new(inner_blobstore_id);
                blobstores
                    .into_iter()
                    .find_map(|(blobstore_id, _, blobstore)| {
                        if blobstore_id == seeked_id {
                            Some(remove_wrapper_blobconfigs(blobstore))
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| {
                        format_err!("could not find a blobstore with id {}", inner_blobstore_id)
                    })
            }
            _ => Err(format_err!(
                "inner-blobstore-id supplied but blobstore is not multiplexed"
            )),
        },
    }
}

async fn get_blobstore(
    fb: FacebookInit,
    storage_config: StorageConfig,
    inner_blobstore_id: Option<u64>,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &BlobstoreOptions,
    config_store: &ConfigStore,
) -> Result<Arc<dyn BlobstoreUnlinkOps>, Error> {
    let blobconfig = get_blobconfig(storage_config.blobstore, inner_blobstore_id)?;

    // TODO: Do this for all blobstores that can support unlink, not just SQLBlob
    let sql_blob = make_sql_blobstore(
        fb,
        blobconfig,
        readonly_storage,
        blobstore_options,
        config_store,
    )
    .await?;

    Ok(Arc::new(sql_blob) as Arc<dyn BlobstoreUnlinkOps>)
}

pub async fn subcommand_blobstore_unlink<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'a>,
    sub_m: &'a ArgMatches<'a>,
) -> Result<(), SubcommandError> {
    let config_store = matches.config_store();
    let (_, config) = args::get_config(config_store, matches)?;
    let storage_config = config.storage_config;
    let inner_blobstore_id = args::get_u64_opt(&sub_m, "inner-blobstore-id");
    let blobstore_options = matches.blobstore_options();

    let readonly_storage = matches.readonly_storage();
    let blobstore = get_blobstore(
        fb,
        storage_config,
        inner_blobstore_id,
        *readonly_storage,
        blobstore_options,
        config_store,
    )
    .await?;

    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let key = sub_m.value_of("KEY").unwrap();

    info!(logger, "using blobstore: {:?}", blobstore);

    blobstore.unlink(&ctx, key).await?;

    Ok(())
}
