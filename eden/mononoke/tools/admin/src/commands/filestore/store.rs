/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bytes::BytesMut;
use clap::Args;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStoreRef;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::stream::TryStreamExt;
use repo_blobstore::RepoBlobstoreRef;
use tokio::io::BufReader;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;

use super::Repo;

#[derive(Args)]
pub struct FilestoreStoreArgs {
    #[clap(parse(from_os_str))]
    file: PathBuf,

    #[clap(long)]
    bubble_id: Option<BubbleId>,
}

pub async fn store(ctx: &CoreContext, repo: &Repo, store_args: FilestoreStoreArgs) -> Result<()> {
    let file = tokio::fs::File::open(&store_args.file)
        .await
        .context("Failed to open file")?;
    let len = file
        .metadata()
        .await
        .context("Failed to get file metadata")?
        .len();
    let blobstore = match store_args.bubble_id {
        Some(bubble_id) => {
            let bubble = repo.repo_ephemeral_store().open_bubble(bubble_id).await?;
            bubble.wrap_repo_blobstore(repo.repo_blobstore().clone())
        }
        None => repo.repo_blobstore().clone(),
    };
    let data = FramedRead::new(BufReader::new(file), BytesCodec::new())
        .map_ok(BytesMut::freeze)
        .map_err(Error::from);
    let metadata = filestore::store(
        &blobstore,
        repo.filestore_config().clone(),
        ctx,
        &StoreRequest::new(len),
        data,
    )
    .await
    .context("Failed to write to filestore")?;

    println!(
        "Wrote {} ({} bytes)",
        metadata.content_id, metadata.total_size
    );

    Ok(())
}
