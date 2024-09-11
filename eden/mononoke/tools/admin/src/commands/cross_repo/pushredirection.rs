/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::Freshness;
use clap::Args;
use clap::Subcommand;
use cmdlib_cross_repo::create_commit_syncers_from_app;
use context::CoreContext;
use cross_repo_sync::Syncers;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCountersRef;
use repo_identity::RepoIdentityRef;
use slog::info;

use super::Repo;

/// Commands to enable/disable pushredirection
#[derive(Args)]
pub struct PushredirectionArgs {
    #[clap(subcommand)]
    subcommand: PushredirectionSubcommand,
}

#[derive(Subcommand)]
pub enum PushredirectionSubcommand {
    /// Command to prepare rollout of pushredirection
    PrepareRollout(PrepareRolloutArgs),
}

#[derive(Args)]
pub struct PrepareRolloutArgs {}

pub async fn pushredirection(
    ctx: &CoreContext,
    app: &MononokeApp,
    source_repo: Repo,
    target_repo: Repo,
    args: PushredirectionArgs,
) -> Result<()> {
    let source_repo = Arc::new(source_repo);
    let target_repo = Arc::new(target_repo);

    let commit_syncers =
        create_commit_syncers_from_app(ctx, app, source_repo.clone(), target_repo.clone()).await?;

    match args.subcommand {
        PushredirectionSubcommand::PrepareRollout(args) => {
            pushredirection_prepare_rollout(ctx, commit_syncers, args).await
        }
    }
}

async fn pushredirection_prepare_rollout(
    ctx: &CoreContext,
    commit_syncers: Syncers<Arc<Repo>>,
    _args: PrepareRolloutArgs,
) -> Result<()> {
    let commit_syncer = commit_syncers.large_to_small;

    if commit_syncer
        .get_live_commit_sync_config()
        .push_redirector_enabled_for_public(
            ctx,
            commit_syncer.get_small_repo().repo_identity().id(),
        )
        .await?
    {
        return Err(anyhow!(
            "not allowed to run prepare-rollout if pushredirection is enabled",
        ));
    }

    let small_repo = commit_syncer.get_small_repo();
    let large_repo = commit_syncer.get_large_repo();

    let largest_id = large_repo
        .bookmark_update_log()
        .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
        .await?
        .ok_or_else(|| anyhow!("No bookmarks update log entries for large repo"))?;

    let counter = backsyncer::format_counter(&large_repo.repo_identity().id());
    info!(
        ctx.logger(),
        "setting value {} to counter {} for repo {}",
        largest_id,
        counter,
        small_repo.repo_identity().id()
    );
    let res = small_repo
        .mutable_counters()
        .set_counter(
            ctx,
            &counter,
            largest_id.try_into().unwrap(),
            None, // prev_value
        )
        .await?;

    if !res {
        Err(anyhow!("failed to set backsyncer counter"))
    } else {
        info!(ctx.logger(), "successfully updated the counter");
        Ok(())
    }
}
