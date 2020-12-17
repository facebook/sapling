/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::{Error, Result};
use cached_config::ConfigStore;
use fbinit::FacebookInit;
use slog::Logger;

use blobstore::Blobstore;
use blobstore_factory::{make_blobstore, BlobstoreOptions};
use metaconfig_types::{ScrubAction, StorageConfig};
use sql_ext::facebook::MysqlOptions;

pub async fn open_blobstore(
    fb: FacebookInit,
    mut storage_config: StorageConfig,
    mysql_options: &MysqlOptions,
    blobstore_options: &BlobstoreOptions,
    logger: &Logger,
    config_store: &ConfigStore,
) -> Result<Arc<dyn Blobstore>> {
    storage_config.blobstore.set_scrubbed(ScrubAction::Repair);

    make_blobstore(
        fb,
        storage_config.blobstore,
        mysql_options,
        blobstore_factory::ReadOnlyStorage(false),
        blobstore_options,
        logger,
        config_store,
    )
    .await
    .map_err(Error::from)
}
