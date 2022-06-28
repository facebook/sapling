/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use context::CoreContext;
use futures::future;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::ContentMetadata;

use crate::Alias;
use crate::FileContents;
use crate::FilestoreConfig;

#[async_trait]
pub trait BlobCopier {
    async fn copy(&self, ctx: &CoreContext, key: String) -> Result<()>;
}

pub async fn copy(
    original_blobstore: impl Blobstore + Clone,
    copier: &impl BlobCopier,
    config: FilestoreConfig,
    ctx: &CoreContext,
    data: &ContentMetadata,
) -> Result<()> {
    // See reasoning about order of writes in ./finalize.rs::finalize (https://fburl.com/code/3w8dncr3)
    let copy_sha1 = copier.copy(ctx, Alias::Sha1(data.sha1).blobstore_key());
    let copy_sha256 = copier.copy(ctx, Alias::Sha256(data.sha256).blobstore_key());
    let copy_git_sha1 = copier.copy(ctx, Alias::GitSha1(data.git_sha1.sha1()).blobstore_key());

    future::try_join3(copy_sha1, copy_sha256, copy_git_sha1).await?;

    // Files are stored inline or in chunks, depending on their size. If they're chunked,
    // we need to copy all chunks. Unfortunately, the only way to know how they're stored is
    // by loading FileContents, which might be large-ish if the file is actually inlined.
    let file_contents = data.content_id.load(ctx, &original_blobstore).await?;
    match file_contents {
        FileContents::Chunked(chunked) => {
            stream::iter(
                chunked
                    .into_chunks()
                    .into_iter()
                    .map(|c| copier.copy(ctx, c.chunk_id().blobstore_key())),
            )
            .buffer_unordered(config.concurrency)
            .try_collect()
            .await?
        }
        FileContents::Bytes(_) => {}
    }

    copier.copy(ctx, data.content_id.blobstore_key()).await?;

    copier
        .copy(ctx, data.clone().into_blob().id().blobstore_key())
        .await?;
    Ok(())
}
