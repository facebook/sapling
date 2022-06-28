/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::ops::Range;

use crate::MononokeSQLBlobGCArgs;
use blobstore_factory::make_sql_blobstore;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use mononoke_app::MononokeApp;
use sqlblob::Sqlblob;

fn remove_wrapper_blobconfigs(mut blob_config: BlobConfig) -> BlobConfig {
    // Pack is a wrapper store - remove it
    while let BlobConfig::Pack { ref blobconfig, .. } = blob_config {
        blob_config = BlobConfig::clone(blobconfig);
    }
    blob_config
}

fn get_blobconfig(
    app: &MononokeApp,
    storage_config_name: String,
    inner_blobstore_id: Option<u64>,
) -> Result<BlobConfig> {
    let blob_config = {
        let storage_config = app
            .storage_configs()
            .storage
            .get(&storage_config_name)
            .context("Requested storage config not found")?;
        storage_config.blobstore.clone()
    };

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

fn get_shard_range(
    blobstore_config: &BlobConfig,
    shard_start: usize,
    shard_count: Option<usize>,
) -> Range<usize> {
    let max_shards = match &blobstore_config {
        BlobConfig::Mysql {
            remote: ShardableRemoteDatabaseConfig::Sharded(config),
        } => config.shard_num.get(),
        _ => 1,
    };

    Range {
        start: shard_start,
        end: shard_start + shard_count.unwrap_or(max_shards),
    }
}

async fn get_sqlblob(app: &MononokeApp, blobstore_config: BlobConfig) -> Result<Sqlblob> {
    let config_store = app.config_store();
    let blobstore_options = app.environment().blobstore_options.clone();

    Ok(make_sql_blobstore(
        app.fb,
        blobstore_config,
        blobstore_factory::ReadOnlyStorage(false),
        &blobstore_options,
        config_store,
    )
    .await?
    .into_inner())
}

pub async fn get_sqlblob_and_shard_range(app: &MononokeApp) -> Result<(Sqlblob, Range<usize>)> {
    let common_args: MononokeSQLBlobGCArgs = app.args()?;
    let blobstore_config = get_blobconfig(
        app,
        common_args.storage_config_name,
        common_args.inner_blobstore_id,
    )?;
    let shard_start = common_args.start_shard;
    let shard_count = common_args.shard_count;
    Ok((
        get_sqlblob(app, blobstore_config.clone()).await?,
        get_shard_range(&blobstore_config, shard_start, shard_count),
    ))
}
