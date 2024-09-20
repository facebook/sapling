/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Loadable;
use case_conflict_skeleton_manifest::RootCaseConflictSkeletonManifestId;
use clap::Args;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::stream::FuturesUnordered;
use futures::TryStreamExt;
use futures_stats::TimedTryFutureExt;
use mononoke_app::args::ChangesetArgs;
use repo_blobstore::RepoBlobstoreRef;
use slog::debug;

use super::Repo;

#[derive(Args)]
pub(super) struct FindNewArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
}

pub(super) async fn find_new(ctx: &CoreContext, repo: &Repo, args: FindNewArgs) -> Result<()> {
    let cs_id = args.changeset_args.resolve_changeset(ctx, repo).await?;

    let ccsm = repo
        .repo_derived_data
        .derive::<RootCaseConflictSkeletonManifestId>(ctx, cs_id)
        .await?
        .into_inner_id()
        .load(ctx, repo.repo_blobstore())
        .await?;

    let parent_ccsms = repo
        .commit_graph()
        .changeset_parents(ctx, cs_id)
        .await?
        .into_iter()
        .map(|parent| async move {
            anyhow::Ok(
                repo.repo_derived_data
                    .derive::<RootCaseConflictSkeletonManifestId>(ctx, parent)
                    .await?
                    .into_inner_id()
                    .load(ctx, repo.repo_blobstore())
                    .await?,
            )
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect::<Vec<_>>()
        .await?;

    let excluded_paths = Default::default();
    let (stats, maybe_case_conflict) = ccsm
        .find_new_case_conflict(ctx, repo.repo_blobstore(), parent_ccsms, &excluded_paths)
        .try_timed()
        .await?;

    if let Some(case_conflict) = maybe_case_conflict {
        println!("Found new case conflict: {:?}", case_conflict);
        debug!(ctx.logger(), "Finished in {:?}", stats.completion_time);
    } else {
        println!("No new case conflicts found");
        debug!(ctx.logger(), "Finished in {:?}", stats.completion_time);
    }

    Ok(())
}
