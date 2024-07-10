/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use changesets_uploader::MononokeCasChangesetsUploader;
use changesets_uploader::UploadPolicy;
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

pub async fn try_sync<'a>(
    re_cas_client: &MononokeCasChangesetsUploader<'a>,
    repo: &'a Repo,
    ctx: &'a CoreContext,
    bcs_id: ChangesetId,
) -> Result<ChangesetId, Error> {
    // Derive augmented manifest for this changeset if not yet derived.
    repo.repo_derived_data()
        .derive::<RootHgAugmentedManifestId>(ctx, bcs_id)
        .await?;
    // Upload changeset to CAS.
    re_cas_client
        .upload_single_changeset(ctx, repo, &bcs_id, UploadPolicy::All)
        .await?;
    Ok(bcs_id)
}

/// Sends commits to CAS while syncing a set of bookmark update log entries.
pub async fn try_sync_single_combined_entry<'a>(
    re_cas_client: &MononokeCasChangesetsUploader<'a>,
    repo: &'a Repo,
    ctx: &'a CoreContext,
    combined_entry: &CombinedBookmarkUpdateLogEntry,
) -> Result<RetryAttemptsCount, Error> {
    let ids: Vec<_> = combined_entry
        .components
        .iter()
        .map(|entry| entry.id)
        .collect();
    info!(ctx.logger(), "syncing log entries {:?} ...", ids);

    let mut queue = Vec::new();

    // Initial implementation process all entries sequentially
    for entry in combined_entry.components.iter() {
        match (entry.from_changeset_id, entry.to_changeset_id) {
            (Some(from_cs_id), Some(to_cs_id)) => {
                let is_ancestor = repo
                    .commit_graph
                    .is_ancestor(ctx, from_cs_id, to_cs_id)
                    .await?;
                if !is_ancestor {
                    // Force non-forward moves are skipped.
                    continue;
                }
            }
            _ => {}
        };

        if entry.to_changeset_id.is_none() {
            info!(
                ctx.logger(),
                "log entry {:?} is a deletion of bookmark, skipping...", &entry
            );
            continue;
        }

        let to = entry.to_changeset_id.unwrap();

        if entry.from_changeset_id.is_none() {
            info!(
                ctx.logger(),
                "log entry {:?} is a creation of bookmark", &entry
            );
            // TODO(liubovd): think about the creation case, how to process correctly
            queue.push(to);
            continue;
        }

        let from = entry.from_changeset_id.unwrap();

        let mut commits = repo
            .commit_graph()
            .range_stream(ctx, from, to)
            .await?
            // Drop from
            .skip(1)
            .collect::<Vec<_>>()
            .await;

        queue.append(&mut commits);
    }

    let commit_num = queue.len();
    stream::iter(queue)
        .map(move |bcs_id| async move { try_sync(re_cas_client, repo, ctx, bcs_id).await })
        .buffered(DEFAULT_UPLOAD_CONCURRENT_COMMITS)
        .try_collect::<Vec<ChangesetId>>()
        .await?;

    info!(
        ctx.logger(),
        "log entries {:?} synced ({} commits uploaded)", ids, commit_num
    );
    // TODO: add configurable retries.
    Ok(RetryAttemptsCount(DEFAULT_UPLOAD_RETRY_NUM))
}
