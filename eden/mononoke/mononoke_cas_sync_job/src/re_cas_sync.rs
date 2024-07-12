/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bookmarks::BookmarkUpdateLogEntry;
use changesets_uploader::MononokeCasChangesetsUploader;
use changesets_uploader::PriorLookupPolicy;
use changesets_uploader::UploadPolicy;
use changesets_uploader::UploadStats;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mercurial_derivation::RootHgAugmentedManifestId;
use mononoke_types::ChangesetId;
use repo_derived_data::RepoDerivedDataRef;
use slog::info;

use crate::CombinedBookmarkUpdateLogEntry;
use crate::Repo;
use crate::RetryAttemptsCount;

const DEFAULT_UPLOAD_RETRY_NUM: usize = 1;
const DEFAULT_UPLOAD_CONCURRENT_COMMITS: usize = 100;
const DEFAULT_DERIVE_CONCURRENT_COMMITS: usize = 100;
const DEFAULT_CONCURRENT_ENTRIES_FOR_COMMIT_GRAPH: usize = 100;

pub async fn try_derive<'a>(
    repo: &'a Repo,
    ctx: &'a CoreContext,
    bcs_id: ChangesetId,
) -> Result<ChangesetId, Error> {
    // Derive augmented manifest for this changeset if not yet derived.
    repo.repo_derived_data()
        .derive::<RootHgAugmentedManifestId>(ctx, bcs_id)
        .await?;
    Ok(bcs_id)
}

pub async fn try_sync<'a>(
    re_cas_client: &MononokeCasChangesetsUploader<'a>,
    repo: &'a Repo,
    ctx: &'a CoreContext,
    bcs_id: ChangesetId,
) -> Result<UploadStats, Error> {
    // Upload changeset to RE CAS.
    // Prior lookup for trees is not requested for performance,
    // since the trees are delta in the incremental sync and should always be new
    // and not present in the CAS before.
    let stats = re_cas_client
        .upload_single_changeset(
            ctx,
            repo,
            &bcs_id,
            UploadPolicy::All,
            PriorLookupPolicy::BlobsOnly,
        )
        .await?;
    Ok(stats)
}

pub async fn try_expand_entry<'a>(
    repo: &'a Repo,
    ctx: &'a CoreContext,
    entry: BookmarkUpdateLogEntry,
) -> Result<Option<Vec<ChangesetId>>, Error> {
    match (entry.from_changeset_id, entry.to_changeset_id) {
        (Some(from_cs_id), Some(to_cs_id)) => {
            let is_ancestor = repo
                .commit_graph
                .is_ancestor(ctx, from_cs_id, to_cs_id)
                .await?;
            if !is_ancestor {
                // Force non-forward moves are skipped.
                return anyhow::Ok(None);
            }
        }
        _ => {}
    };
    if entry.to_changeset_id.is_none() {
        info!(
            ctx.logger(),
            "log entry {:?} is a deletion of bookmark, skipping...", &entry
        );
        return Ok(None);
    }
    let to = entry.to_changeset_id.unwrap();
    if entry.from_changeset_id.is_none() {
        info!(
            ctx.logger(),
            "log entry {:?} is a creation of bookmark", &entry
        );
        // TODO(liubovd): think about the creation case, how to process correctly
        return Ok(Some(vec![to]));
    }
    let from = entry.from_changeset_id.unwrap();
    Ok(Some(
        repo.commit_graph()
            .range_stream(ctx, from, to)
            .await?
            // Drop from
            .skip(1)
            .collect::<Vec<_>>()
            .await,
    ))
}

/// Sends commits to CAS while syncing a list of bookmark update log entries.
pub async fn try_sync_single_combined_entry<'a>(
    re_cas_client: &MononokeCasChangesetsUploader<'a>,
    repo: &'a Repo,
    ctx: &'a CoreContext,
    combined_entry: &'a CombinedBookmarkUpdateLogEntry,
) -> Result<RetryAttemptsCount, Error> {
    let ids: Vec<_> = combined_entry
        .components
        .iter()
        .map(|entry| entry.id)
        .collect();
    info!(ctx.logger(), "syncing log entries {:?} ...", ids);

    let start_time = std::time::Instant::now();
    let queue: Vec<ChangesetId> = futures::stream::iter(combined_entry.components.clone())
        .map(|entry| async move { try_expand_entry(repo, ctx, entry).await })
        .buffered(DEFAULT_CONCURRENT_ENTRIES_FOR_COMMIT_GRAPH)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .filter_map(|res| res)
        .flatten()
        .collect();

    // Order is important for derivation of augmented manifests.
    let derived = stream::iter(queue)
        .map(move |bcs_id| async move { try_derive(repo, ctx, bcs_id).await })
        .buffered(DEFAULT_DERIVE_CONCURRENT_COMMITS)
        .try_collect::<Vec<ChangesetId>>()
        .await?;

    // Once everything is derived, the upload order does not matter.
    let uploaded_len = derived.len();
    let upload_stats = stream::iter(derived)
        .map(move |bcs_id| async move { try_sync(re_cas_client, repo, ctx, bcs_id).await })
        .buffer_unordered(DEFAULT_UPLOAD_CONCURRENT_COMMITS)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .fold(
            UploadStats::default(),
            |acc: UploadStats, current: UploadStats| {
                acc.add(current.as_ref());
                acc
            },
        );

    info!(
        ctx.logger(),
        "log entries {:?} synced ({} commits uploaded, upload stats: {}), took overall {:.3} sec",
        ids,
        uploaded_len,
        upload_stats,
        start_time.elapsed().as_secs_f64(),
    );
    // TODO: add configurable retries.
    Ok(RetryAttemptsCount(DEFAULT_UPLOAD_RETRY_NUM))
}
