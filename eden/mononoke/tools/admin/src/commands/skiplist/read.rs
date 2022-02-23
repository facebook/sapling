/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::repo::AdminRepo;
use anyhow::{Error, Result};
use blobstore::Blobstore;
use context::CoreContext;
use skiplist::{deserialize_skiplist_index, SkiplistIndex};
use slog::{debug, info, Logger};

pub async fn read_skiplist(
    ctx: &CoreContext,
    repo: &AdminRepo,
    logger: &Logger,
    blobstore_key: String,
) -> Result<()> {
    let maybe_index = get_skiplist_index(ctx, repo, logger, blobstore_key).await?;
    match maybe_index {
        Some(index) => {
            info!(
                logger,
                "skiplist graph has {} entries",
                index.indexed_node_count()
            );
        }
        None => {
            info!(logger, "skiplist not found");
        }
    };
    Ok(())
}

pub async fn get_skiplist_index(
    ctx: &CoreContext,
    repo: &AdminRepo,
    logger: &Logger,
    blobstore_key: String,
) -> Result<Option<SkiplistIndex>, Error> {
    let maybebytes = repo.repo_blobstore.get(ctx, &blobstore_key).await?;
    match maybebytes {
        Some(bytes) => {
            debug!(
                logger,
                "received {} bytes from blobstore",
                bytes.as_bytes().len()
            );
            let bytes = bytes.into_raw_bytes();
            Ok(Some(deserialize_skiplist_index(logger.clone(), bytes)?))
        }
        None => Ok(None),
    }
}
