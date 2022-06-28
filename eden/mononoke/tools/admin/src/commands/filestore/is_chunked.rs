/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use clap::Args;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStoreRef;
use mononoke_types::FileContents;
use repo_blobstore::RepoBlobstoreRef;

use super::FilestoreItemIdArgs;
use super::Repo;

#[derive(Args)]
pub struct FilestoreIsChunkedArgs {
    #[clap(flatten)]
    item_id: FilestoreItemIdArgs,

    #[clap(long)]
    bubble_id: Option<BubbleId>,
}

pub async fn is_chunked(
    ctx: &CoreContext,
    repo: &Repo,
    metadata_args: FilestoreIsChunkedArgs,
) -> Result<()> {
    let fetch_key = metadata_args.item_id.fetch_key()?;
    let blobstore = match metadata_args.bubble_id {
        Some(bubble_id) => {
            let bubble = repo.repo_ephemeral_store().open_bubble(bubble_id).await?;
            bubble.wrap_repo_blobstore(repo.repo_blobstore().clone())
        }
        None => repo.repo_blobstore().clone(),
    };
    let metadata = filestore::get_metadata(&blobstore, ctx, &fetch_key)
        .await
        .context("Failed to get metadata from filestore")?;

    match metadata {
        Some(metadata) => {
            let file_contents = metadata
                .content_id
                .load(ctx, &blobstore)
                .await
                .context("Failed to load content blob")?;
            match file_contents {
                FileContents::Bytes(_) => {
                    println!("not chunked");
                }
                FileContents::Chunked(_) => {
                    println!("chunked");
                }
            }
        }
        None => {
            println!("content id not found");
        }
    }

    Ok(())
}
