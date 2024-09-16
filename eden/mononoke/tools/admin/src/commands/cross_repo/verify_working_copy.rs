/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use cmdlib_cross_repo::create_commit_syncers_from_app;
use commit_id::parse_commit_id;
use context::CoreContext;
use mononoke_app::MononokeApp;

use super::Repo;

/// Perform working copy verification
#[derive(Args)]
pub struct VerifyWorkingCopyArgs {
    /// Commit id from large repo to verify
    large_repo_commit_id: String,
}

pub async fn verify_working_copy(
    ctx: &CoreContext,
    app: &MononokeApp,
    source_repo: Repo,
    target_repo: Repo,
    args: VerifyWorkingCopyArgs,
) -> Result<()> {
    let commit_syncers = create_commit_syncers_from_app(ctx, app, source_repo, target_repo).await?;
    let commit_syncer = commit_syncers.large_to_small;

    let large_repo_cs_id = parse_commit_id(
        ctx,
        commit_syncer.get_large_repo(),
        &args.large_repo_commit_id,
    )
    .await?;

    cross_repo_sync::verify_working_copy(
        ctx,
        &commit_syncer,
        large_repo_cs_id,
        commit_syncer.get_live_commit_sync_config().clone(),
    )
    .await
}
