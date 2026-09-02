/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use blobstore::KeyedBlobstore;
use cloned::cloned;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future;
use history_manifest::RootHistoryManifestDirectoryId;
use manifest::CombinedId;
use manifest::ManifestOps;
use mononoke_macros::mononoke;
use mononoke_types::FileUnodeId;
use mononoke_types::NonRootMPath;
use mononoke_types::blame_v2::BlameV2Id;
use mononoke_types::blame_v3::BlameV3Id;
use mononoke_types::typed_hash::HistoryManifestFileId;

use crate::mapping_v2::RootBlameV2;

/// Number of paths whose blame blob is copied concurrently.
const COPY_CONCURRENCY: usize = 256;

/// Derive blame v3 for a changeset by re-keying its blame v2 blobs.
///
/// v2 and v3 hold the same payload under different keys — `FileUnodeId` versus
/// `HistoryManifestFileId` — and neither id is recoverable from the other, so
/// each path's pair comes from walking both manifests together.
///
/// Every live path is copied, not just the ones this changeset modified: a
/// descendant reads its parent blame under the id minted by whichever ancestor
/// last touched the path, and at a slice boundary that ancestor may be in a
/// slice that has not been derived yet.
pub(crate) async fn derive_blame_v3_from_predecessor(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    blame_v2: RootBlameV2,
    root_manifest: RootHistoryManifestDirectoryId,
) -> Result<(), Error> {
    let hm_root = root_manifest.into_history_manifest_directory_id();
    let unode_root = *blame_v2.root_manifest().manifest_unode_id();

    // The history manifest's `Manifest` impl filters out its deleted nodes, so
    // both sides list the same live file set and pair by position.
    //
    // Spawned rather than only buffered, like `derive_blame_v2`: each copy
    // compresses and decompresses through packblob, which would otherwise all
    // land on one task.
    let copy = CombinedId(hm_root, unode_root)
        .list_leaf_entries(ctx.clone(), blobstore.clone())
        .map(|entry| {
            cloned!(ctx, blobstore);
            async move {
                let (path, CombinedId(hm_file_id, file_unode_id)) = entry?;
                mononoke::spawn_task(async move {
                    copy_blame(&ctx, &blobstore, path, file_unode_id, hm_file_id).await
                })
                .await?
            }
        })
        .buffer_unordered(COPY_CONCURRENCY)
        .try_fold(0usize, |count, ()| future::ok(count + 1));

    // `Combined::list` zips the two streams, so a length mismatch truncates
    // silently instead of erroring, leaving paths with no blob while the
    // mapping is still stored. These walks are metadata only.
    let hm_leaves = hm_root
        .list_leaf_entries(ctx.clone(), blobstore.clone())
        .try_fold(0usize, |count, _| future::ok(count + 1));
    let unode_leaves = unode_root
        .list_leaf_entries(ctx.clone(), blobstore.clone())
        .try_fold(0usize, |count, _| future::ok(count + 1));

    let (copied, hm_leaves, unode_leaves) =
        future::try_join3(copy, hm_leaves, unode_leaves).await?;

    if copied != hm_leaves || copied != unode_leaves {
        bail!(
            "blame v3 transcode covered {copied} paths, but the history manifest has \
             {hm_leaves} leaves and the unode manifest has {unode_leaves}"
        );
    }

    Ok(())
}

/// Copy a blame blob from its v2 key to its v3 key.
///
/// Raw bytes: both versions compact-protocol-serialize the same `BlameV2`
/// thrift, so decoding and re-encoding would only burn CPU.
async fn copy_blame(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    path: NonRootMPath,
    file_unode_id: FileUnodeId,
    hm_file_id: HistoryManifestFileId,
) -> Result<(), Error> {
    let v2_key = BlameV2Id::from(file_unode_id).blobstore_key();
    let blame = blobstore
        .get(ctx, &v2_key)
        .await?
        .ok_or_else(|| anyhow!("blame v2 missing for {path} at key {v2_key}"))?;

    blobstore
        .put(
            ctx,
            BlameV3Id::from(hm_file_id).blobstore_key(),
            blame.into_bytes(),
        )
        .await
}
