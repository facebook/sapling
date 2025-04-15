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
use mononoke_types::BlobstoreValue;
use mononoke_types::SkeletonManifestId;
use mononoke_types::case_conflict_skeleton_manifest::CaseConflictSkeletonManifest;
use skeleton_manifest::RootSkeletonManifestId;

use crate::derive::inner_derive;
use crate::mapping::RootCaseConflictSkeletonManifestId;
use crate::path::CcsmPath;

pub(crate) async fn inner_derive_from_predecessor(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    predecessor: SkeletonManifestId,
    chunk_size: usize,
) -> Result<CaseConflictSkeletonManifest> {
    predecessor
        .list_leaf_entries(ctx.clone(), blobstore.clone())
        .try_filter_map(|(path, ())| async move {
            Ok(CcsmPath::transform(path).map(|path| (path.into_raw(), Some(()))))
        })
        .try_chunks(chunk_size)
        .map_err(anyhow::Error::msg)
        .try_fold(CaseConflictSkeletonManifest::empty(), |ccsm, paths| {
            cloned!(ctx, blobstore);
            async move {
                let new_ccsm = inner_derive(&ctx, &blobstore, vec![ccsm], paths, Vec::new())
                    .await?
                    .unwrap_or_else(CaseConflictSkeletonManifest::empty);
                Ok(new_ccsm)
            }
        })
        .await
}

pub(crate) async fn derive_from_predecessor(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    predecessor: RootSkeletonManifestId,
) -> Result<RootCaseConflictSkeletonManifestId> {
    let blobstore = derivation_ctx.blobstore();
    let predecessor = predecessor.into_skeleton_manifest_id();

    let chunk_size =
        justknobs::get_as::<usize>("scm/mononoke:ccsm_derive_from_predecessor_chunk_size", None)?;
    let directory = inner_derive_from_predecessor(ctx, blobstore, predecessor, chunk_size).await?;

    Ok(RootCaseConflictSkeletonManifestId(
        directory
            .into_blob()
            .store(ctx, &blobstore)
            .await
            .context("failed to store CaseConflictSkeletonManifest blob")?,
    ))
}
