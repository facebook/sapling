/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use cmdlib_cross_repo::create_commit_syncers_from_app;
use context::CoreContext;
use cross_repo_sync::UpdateLargeRepoBookmarksMode;
use cross_repo_sync::VerifyBookmarksRunMode;
use mononoke_app::MononokeApp;

use super::Repo;

/// Verify that bookmarks are the same in small and large repo (subject to bookmark renames)
#[derive(Args)]
pub struct VerifyBookmarksArgs {
    /// Don't do actual bookmark updates, only print what would be done (deriving data is real!)
    #[clap(long)]
    no_bookmark_updates: bool,

    /// Update any inconsistencies between bookmarks (except for the common bookmarks between large and small repo e.g. 'master')
    #[clap(long)]
    update_large_repo_bookmarks: bool,

    /// Limit on number of bookmarks to update in the large repo. Default is no limit.
    #[clap(long)]
    limit: Option<usize>,
}

pub async fn verify_bookmarks(
    ctx: &CoreContext,
    app: &MononokeApp,
    source_repo: Repo,
    target_repo: Repo,
    args: VerifyBookmarksArgs,
) -> Result<()> {
    let commit_syncers = create_commit_syncers_from_app(ctx, app, source_repo, target_repo).await?;

    let mode = if args.update_large_repo_bookmarks {
        VerifyBookmarksRunMode::UpdateLargeRepoBookmarks {
            mode: if args.no_bookmark_updates {
                UpdateLargeRepoBookmarksMode::DryRun
            } else {
                UpdateLargeRepoBookmarksMode::Real
            },
            limit: args.limit,
        }
    } else {
        VerifyBookmarksRunMode::JustVerify
    };

    cross_repo_sync::verify_bookmarks(ctx, commit_syncers, mode).await
}
