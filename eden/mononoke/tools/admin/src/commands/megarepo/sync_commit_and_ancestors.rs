/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::format_err;
use cmdlib_cross_repo::create_single_direction_commit_syncer;
use commit_id::parse_commit_id;
use context::CoreContext;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::Repo as CrossRepo;
use cross_repo_sync::find_toposorted_unsynced_ancestors;
use cross_repo_sync::unsafe_sync_commit;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceAndTargetRepoArgs;
use slog::info;

/// Command that syncs a commit and all of its unsynced ancestors from source repo
/// to target repo. This is similar to SCS commit_lookup_xrepo() method except that it
/// doesn't do all the safety checks that commit_lookup_xrepo(). In particular, it allows
/// to sync public small repo commits.
#[derive(Debug, clap::Args)]
pub struct SyncCommitAndAncestorsArgs {
    #[clap(flatten)]
    repo_args: SourceAndTargetRepoArgs,

    /// Commit (and its ancestors) to sync
    #[clap(long)]
    commit_hash: String,
}

pub async fn run(
    ctx: &CoreContext,
    app: MononokeApp,
    args: SyncCommitAndAncestorsArgs,
) -> Result<()> {
    let source_repo: Repo = app.open_repo(&args.repo_args.source_repo).await?;
    info!(
        ctx.logger(),
        "using repo \"{}\" repoid {:?}",
        source_repo.repo_identity().name(),
        source_repo.repo_identity().id()
    );

    let target_repo: Repo = app.open_repo(&args.repo_args.target_repo).await?;
    info!(
        ctx.logger(),
        "using repo \"{}\" repoid {:?}",
        target_repo.repo_identity().name(),
        target_repo.repo_identity().id()
    );

    let commit_sync_data =
        create_single_direction_commit_syncer(ctx, &app, source_repo.clone(), target_repo.clone())
            .await?;
    let source_cs = parse_commit_id(ctx, &source_repo, &args.commit_hash).await?;
    info!(ctx.logger(), "changeset resolved as: {:?}", source_cs);

    let (unsynced_ancestors, _) =
        find_toposorted_unsynced_ancestors(ctx, &commit_sync_data, source_cs, None).await?;

    for ancestor in unsynced_ancestors {
        unsafe_sync_commit(
            ctx,
            ancestor,
            &commit_sync_data,
            CandidateSelectionHint::Only,
            CommitSyncContext::AdminChangeMapping,
            None,
            false, // add_mapping_to_hg_extra
        )
        .await?;
    }

    let commit_sync_outcome = commit_sync_data
        .get_commit_sync_outcome(ctx, source_cs)
        .await?
        .ok_or_else(|| format_err!("was not able to remap a commit {}", source_cs))?;
    info!(ctx.logger(), "remapped to {:?}", commit_sync_outcome);

    Ok(())
}
