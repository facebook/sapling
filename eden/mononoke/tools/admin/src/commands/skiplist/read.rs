/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::Repo;
use anyhow::Error;
use anyhow::Result;
use blobstore::Blobstore;
use clap::Args;
use context::CoreContext;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreRef;
use skiplist::deserialize_skiplist_index;
use skiplist::SkiplistIndex;
use slog::debug;
use slog::Logger;

#[derive(Args)]
/// Subcommand to build skiplist indexes.
pub struct SkiplistReadArgs {
    /// Show entries for these changesets.
    #[clap(long, short = 's')]
    show: Vec<ChangesetId>,
}

pub async fn read_skiplist(
    ctx: &CoreContext,
    repo: &Repo,
    logger: &Logger,
    blobstore_key: String,
    args: SkiplistReadArgs,
) -> Result<()> {
    let maybe_index = get_skiplist_index(ctx, repo, logger, blobstore_key).await?;
    match maybe_index {
        Some(index) => {
            println!("Skiplist graph has {} entries", index.indexed_node_count());
            for cs_id in args.show {
                println!("{}: {:?}", cs_id, index.get_furthest_edges(cs_id));
            }
        }
        None => {
            println!("Skiplist not found");
        }
    };
    Ok(())
}

pub async fn get_skiplist_index(
    ctx: &CoreContext,
    repo: &Repo,
    logger: &Logger,
    blobstore_key: String,
) -> Result<Option<SkiplistIndex>, Error> {
    let maybebytes = repo.repo_blobstore().get(ctx, &blobstore_key).await?;
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
