/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use blobstore::BlobCopier;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::Storable;
use context::CoreContext;
use futures::future;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::ContentAlias;
use mononoke_types::ContentMetadataV2;
use slog::info;
use strum::IntoEnumIterator;

use crate::Alias;
use crate::AliasBlob;
use crate::FileContents;
use crate::FilestoreConfig;

pub async fn copy(
    original_blobstore: &impl Blobstore,
    copier: &(impl BlobCopier + Sync),
    config: FilestoreConfig,
    ctx: &CoreContext,
    data: &ContentMetadataV2,
) -> Result<()> {
    // See reasoning about order of writes in ./finalize.rs::finalize (https://fburl.com/code/3w8dncr3)

    // Ensure that all aliases are covered, and missing out an alias gives a compile time error.
    future::try_join_all(Alias::iter().map(|alias| {
        match alias {
            Alias::Sha1(_) => copier.copy(ctx, Alias::Sha1(data.sha1).blobstore_key()),
            Alias::GitSha1(_) => {
                copier.copy(ctx, Alias::GitSha1(data.git_sha1.sha1()).blobstore_key())
            }
            Alias::Sha256(_) => copier.copy(ctx, Alias::Sha256(data.sha256).blobstore_key()),
            Alias::SeededBlake3(_) => Box::pin(async move {
                let blake3 = Alias::SeededBlake3(data.seeded_blake3);
                match copier.copy(ctx, blake3.blobstore_key()).await
                {
                    resp @ Ok(_) => resp,
                    // The backfilling for ContentMetadataV2 has happened in different stages so the alias#
                    // might be missing. Regenerate it.
                    Err(_) => {
                        info!(
                            ctx.logger(),
                            "Failure in copying seeded blake3 ({:?}) alias for content ID {:?}. Generating Alias.",
                            data.seeded_blake3,
                            data.content_id
                        );
                        let content_alias = ContentAlias::from_content_id(data.content_id);
                        AliasBlob(blake3, content_alias).store(ctx, original_blobstore).await?;
                        // Now that the alias has been regenerated, try copying again.
                        copier.copy(ctx, blake3.blobstore_key()).await
                    }
                }
            })
        }
    }))
    .await
    .with_context(|| {
        format!(
            "Failure in copying alias for content id {:?}",
            data.content_id
        )
    })?;

    // Files are stored inline or in chunks, depending on their size. If they're chunked,
    // we need to copy all chunks. Unfortunately, the only way to know how they're stored is
    // by loading FileContents, which might be large-ish if the file is actually inlined.
    let file_contents = data.content_id.load(ctx, original_blobstore).await?;
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
