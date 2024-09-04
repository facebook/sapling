/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogEntry;
use cas_client::CasClient;
use changesets_uploader::CasChangesetsUploader;
use changesets_uploader::PriorLookupPolicy;
use changesets_uploader::UploadPolicy;
use changesets_uploader::UploadStats;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedTryFutureExt;
use futures_watchdog::WatchdogExt;
use itertools::Itertools;
use mercurial_derivation::RootHgAugmentedManifestId;
use mononoke_types::ChangesetId;
use repo_derived_data::RepoDerivedDataRef;
use slog::debug;
use slog::error;
use slog::info;

use crate::CombinedBookmarkUpdateLogEntry;
use crate::Repo;
use crate::RetryAttemptsCount;

const DEFAULT_UPLOAD_RETRY_NUM: usize = 1;
const DEFAULT_UPLOAD_CONCURRENT_COMMITS: usize = 100;
const DEFAULT_CONCURRENT_ENTRIES_FOR_COMMIT_GRAPH: usize = 100;
const DEFAULT_MAX_COMMITS_PER_BOOKMARK_CREATION: u64 = 1000;

pub async fn try_expand_bookmark_creation_entry<'a>(
    re_cas_client: &CasChangesetsUploader<impl CasClient + 'a>,
    repo: &'a Repo,
    ctx: &'a CoreContext,
    to_bcs_id: ChangesetId,
    main_bookmark: &'a str,
    bookmark: BookmarkKey,
) -> Result<Vec<ChangesetId>, Error> {
    let bookmark_name = bookmark.as_str();
    let frontier = repo
        .commit_graph()
        .ancestors_frontier_with(ctx, vec![to_bcs_id], move |bcs_id| async move {
            if bookmark_name != main_bookmark {
                re_cas_client
                    .is_changeset_uploaded(ctx, repo, &bcs_id)
                    .await
                    .map_err(Error::from)
            } else {
                // Do not rely on the lookups for the main bookmark.
                Ok(false)
            }
        })
        .await?;

    // Estimate the number of commits to upload for the bookmark creation entry.
    // Runs in O((heads + common) * log) regardless of stream size
    let estimate: u64 = repo
        .commit_graph()
        .ancestors_difference_segments(ctx, vec![to_bcs_id], frontier.clone())
        .await?
        .into_iter()
        .map(|segment| segment.length)
        .sum();

    if estimate > DEFAULT_MAX_COMMITS_PER_BOOKMARK_CREATION {
        error!(
            ctx.logger(),
            "Too many commits to upload for the bookmark creation entry {}: {}, limit is {}. Please, consider recursive uploading a revision with mononoke_newadmin instead",
            bookmark,
            estimate,
            DEFAULT_MAX_COMMITS_PER_BOOKMARK_CREATION
        );
        // Upload only the bookmark creation commit. The working copy for this commit will have gaps in CAS.
        return Ok(vec![to_bcs_id]);
    }

    repo.commit_graph()
        .ancestors_difference_stream(ctx, vec![to_bcs_id], frontier)
        .await?
        .try_collect::<Vec<_>>()
        .await
}

pub async fn try_derive<'a>(
    repo: &'a Repo,
    ctx: &'a CoreContext,
    bcs_id: ChangesetId,
) -> Result<(), Error> {
    // Derive augmented manifest for this changeset if not yet derived.
    repo.repo_derived_data()
        .derive::<RootHgAugmentedManifestId>(ctx, bcs_id)
        .await?;
    Ok(())
}

pub async fn try_sync<'a>(
    re_cas_client: &CasChangesetsUploader<impl CasClient + 'a>,
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
    re_cas_client: &CasChangesetsUploader<impl CasClient + 'a>,
    repo: &'a Repo,
    ctx: &'a CoreContext,
    main_bookmark: &'a str,
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

    // Let's double check that all the blobs for the main bookmark are uploaded, and do not rely on the lookups, they are never expected to be hit anyway.
    if entry.bookmark_name.as_str() != main_bookmark
        && re_cas_client.is_changeset_uploaded(ctx, repo, &to).await?
    {
        // Many bookmarks are moved to already uploaded commits, so we can skip them (like stable)
        debug!(
            ctx.logger(),
            "log entry {:?} is a move of bookmark to already uploaded commit, skipping...", &entry
        );
        return Ok(None);
    }

    if entry.from_changeset_id.is_none() {
        info!(
            ctx.logger(),
            "log entry {:?} is a creation of bookmark", &entry
        );
        return Ok(Some(
            try_expand_bookmark_creation_entry(
                re_cas_client,
                repo,
                ctx,
                to,
                main_bookmark,
                entry.bookmark_name,
            )
            .await?,
        ));
    }

    let from = entry.from_changeset_id.unwrap();

    // The sync wasn't started from the beginning of the repo, so we may encounter this bookmark first time. Let's treat it as creation.
    // This can also happen if the gap between the update exceeds our TTL (1 year)
    // We know this can't happen with the main bookmark, skip the check for it to avoid unnecessary CAS lookups.
    if entry.bookmark_name.as_str() != main_bookmark
        && !re_cas_client
            .is_changeset_uploaded(ctx, repo, &from)
            .await?
    {
        info!(
            ctx.logger(),
            "log entry {:?} is a not creation of bookmark, however we sync it first time or it was updated last time more than our TTL",
            &entry
        );
        return Ok(Some(
            try_expand_bookmark_creation_entry(
                re_cas_client,
                repo,
                ctx,
                to,
                main_bookmark,
                entry.bookmark_name,
            )
            .await?,
        ));
    }

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
    re_cas_client: &CasChangesetsUploader<impl CasClient + 'a>,
    repo: &'a Repo,
    ctx: &'a CoreContext,
    combined_entry: &'a CombinedBookmarkUpdateLogEntry,
    main_bookmark: &'a str,
) -> Result<RetryAttemptsCount, Error> {
    let ids: Vec<_> = combined_entry
        .components
        .iter()
        .map(|entry| entry.id)
        .collect();
    info!(ctx.logger(), "syncing log entries {:?} ...", ids);

    let start_time = std::time::Instant::now();
    let queue: Vec<ChangesetId> = futures::stream::iter(combined_entry.components.clone())
        .map(|entry| async move {
            try_expand_entry(re_cas_client, repo, ctx, main_bookmark, entry).await
        })
        .buffer_unordered(DEFAULT_CONCURRENT_ENTRIES_FOR_COMMIT_GRAPH)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .filter_map(|res| res)
        .flatten()
        .unique()
        .collect();

    // Derive augmented manifests for all commits in the queue if not yet derived.
    let (derivation_stats, _) = repo
        .commit_graph()
        .process_topologically(ctx, queue.clone(), move |bcs_id| async move {
            try_derive(repo, ctx, bcs_id).await
        })
        .try_timed()
        .await?;

    // Once everything is derived, the upload order does not matter.
    let uploaded_len = queue.len();
    let upload_stats = stream::iter(queue)
        .map(move |bcs_id| async move { try_sync(re_cas_client, repo, ctx, bcs_id).await })
        .buffer_unordered(DEFAULT_UPLOAD_CONCURRENT_COMMITS)
        .try_collect::<Vec<_>>()
        .watched(ctx.logger())
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
        "log entries {:?} synced ({} commits uploaded, upload stats: {}), took overall {:.3} sec, derivation checks took {:.3} sec",
        ids,
        uploaded_len,
        upload_stats,
        start_time.elapsed().as_secs_f64(),
        derivation_stats.completion_time.as_secs_f64(),
    );
    // TODO: add configurable retries.
    Ok(RetryAttemptsCount(DEFAULT_UPLOAD_RETRY_NUM))
}
