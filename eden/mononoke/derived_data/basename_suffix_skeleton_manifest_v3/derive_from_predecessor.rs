/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use basename_suffix_skeleton_manifest::BssmPath;
use blobstore::Blobstore;
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::basename_suffix_skeleton_manifest_v3::BssmV3Directory;
use mononoke_types::BlobstoreValue;
use mononoke_types::SkeletonManifestId;
use skeleton_manifest::RootSkeletonManifestId;

use crate::derive::inner_derive;
use crate::mapping::RootBssmV3DirectoryId;

pub(crate) async fn inner_derive_from_predecessor(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    predecessor: SkeletonManifestId,
    chunk_size: usize,
) -> Result<BssmV3Directory> {
    predecessor
        .list_leaf_entries(ctx.clone(), blobstore.clone())
        .map_ok(|(path, ())| (BssmPath::transform(path).into_raw(), Some(())))
        .try_chunks(chunk_size)
        .map_err(anyhow::Error::msg)
        .try_fold(BssmV3Directory::empty(), |bssm_v3_directory, paths| {
            cloned!(ctx, blobstore);
            async move {
                let new_bssm_v3_directory =
                    inner_derive(&ctx, &blobstore, vec![bssm_v3_directory], paths)
                        .await?
                        .unwrap_or_else(BssmV3Directory::empty);
                Ok(new_bssm_v3_directory)
            }
        })
        .await
}

pub(crate) async fn derive_from_predecessor(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    predecessor: RootSkeletonManifestId,
) -> Result<RootBssmV3DirectoryId> {
    let blobstore = derivation_ctx.blobstore();
    let predecessor = predecessor.into_skeleton_manifest_id();

    let directory = inner_derive_from_predecessor(ctx, blobstore, predecessor, 100000).await?;

    Ok(RootBssmV3DirectoryId(
        directory
            .into_blob()
            .store(ctx, &blobstore)
            .await
            .context("failed to store BssmV3Directory blob")?,
    ))
}
