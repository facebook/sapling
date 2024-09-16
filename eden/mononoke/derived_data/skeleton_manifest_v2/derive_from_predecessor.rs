/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::skeleton_manifest_v2::SkeletonManifestV2;
use mononoke_types::BlobstoreValue;
use mononoke_types::SkeletonManifestId;
use skeleton_manifest::RootSkeletonManifestId;

use crate::derive::inner_derive;
use crate::mapping::RootSkeletonManifestV2Id;

pub(crate) async fn inner_derive_from_predecessor(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    predecessor: SkeletonManifestId,
    chunk_size: usize,
) -> Result<SkeletonManifestV2> {
    predecessor
        .list_leaf_entries(ctx.clone(), blobstore.clone())
        .map_ok(|(path, ())| (path, Some(())))
        .try_chunks(chunk_size)
        .map_err(anyhow::Error::msg)
        .try_fold(
            SkeletonManifestV2::empty(),
            |skeleton_manifest_v2, paths| {
                cloned!(ctx, blobstore);
                async move {
                    let new_skeleton_manifest_v2 =
                        inner_derive(&ctx, &blobstore, vec![skeleton_manifest_v2], paths)
                            .await?
                            .unwrap_or_else(SkeletonManifestV2::empty);
                    Ok(new_skeleton_manifest_v2)
                }
            },
        )
        .await
}

pub(crate) async fn derive_from_predecessor(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    predecessor: RootSkeletonManifestId,
) -> Result<RootSkeletonManifestV2Id> {
    let blobstore = derivation_ctx.blobstore();
    let predecessor = predecessor.into_skeleton_manifest_id();

    let chunk_size = justknobs::get_as::<usize>(
        "scm/mononoke:skeleton_manifest_v2_derive_from_predecessor_chunk_size",
        None,
    )?;
    let manifest = inner_derive_from_predecessor(ctx, blobstore, predecessor, chunk_size).await?;

    Ok(RootSkeletonManifestV2Id(
        manifest
            .into_blob()
            .store(ctx, &blobstore)
            .await
            .context("failed to store SkeletonManifestV2 blob")?,
    ))
}
