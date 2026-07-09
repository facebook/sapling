/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::stream::TryStreamExt;
use history_manifest::RootHistoryManifestDirectoryId;
use manifest::Entry;
use manifest::find_intersection_of_diffs;
use mononoke_macros::mononoke;
use mononoke_types::BonsaiChangeset;
use mononoke_types::history_manifest::HistoryManifestDirectory;
use mononoke_types::history_manifest::HistoryManifestEntry;
use mononoke_types::history_manifest::HistoryManifestFile;
use mononoke_types::typed_hash::HistoryManifestDirectoryId;
use mononoke_types::typed_hash::HistoryManifestFileId;

use crate::fastlog_impl::create_new_batch_v2;
use crate::fastlog_impl::save_fastlog_batch_by_hm_id;

pub(crate) async fn derive_fastlog_v2(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: BonsaiChangeset,
    root_manifest: RootHistoryManifestDirectoryId,
) -> Result<(), Error> {
    let bcs_id = bonsai.get_changeset_id();
    let hm_dir_id = root_manifest.into_history_manifest_directory_id();
    let parents = derivation_ctx
        .fetch_parents::<RootHistoryManifestDirectoryId>(ctx, &bonsai)
        .await?
        .into_iter()
        .map(|id| id.into_history_manifest_directory_id())
        .collect::<Vec<_>>();

    let blobstore = derivation_ctx.blobstore();

    find_intersection_of_diffs(ctx.clone(), blobstore.clone(), hm_dir_id, parents)
        .map_ok(move |(_, entry)| {
            cloned!(blobstore, ctx);
            async move {
                mononoke::spawn_task(async move {
                    let hm_parents = fetch_hm_parents(&ctx, &blobstore, entry).await?;

                    let fastlog_batch =
                        create_new_batch_v2(&ctx, &blobstore, hm_parents, bcs_id).await?;

                    save_fastlog_batch_by_hm_id(&ctx, &blobstore, entry, fastlog_batch).await
                })
                .await?
            }
        })
        .try_buffer_unordered(100)
        .try_for_each(|_| async { Ok(()) })
        .await?;

    Ok(())
}

async fn fetch_hm_parents(
    ctx: &CoreContext,
    blobstore: &Arc<dyn KeyedBlobstore>,
    hm_entry: Entry<HistoryManifestDirectoryId, HistoryManifestFileId>,
) -> Result<Vec<Entry<HistoryManifestDirectoryId, HistoryManifestFileId>>, Error> {
    let parents = match hm_entry {
        Entry::Tree(dir_id) => {
            let dir: HistoryManifestDirectory = dir_id.load(ctx, blobstore).await?;
            dir.parents.clone()
        }
        Entry::Leaf(file_id) => {
            let file: HistoryManifestFile = file_id.load(ctx, blobstore).await?;
            file.parents.clone()
        }
    };
    Ok(parents
        .into_iter()
        .filter_map(|p| match p {
            HistoryManifestEntry::Directory(id) => Some(Entry::Tree(id)),
            HistoryManifestEntry::File(id) => Some(Entry::Leaf(id)),
            // FastlogBatch does not track deletions; deleted-node parents are dropped.
            HistoryManifestEntry::DeletedNode(_) => None,
        })
        .collect())
}
