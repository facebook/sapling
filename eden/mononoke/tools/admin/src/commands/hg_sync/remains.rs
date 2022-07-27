/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarkUpdateReason;
use clap::Args;
use context::CoreContext;
use mutable_counters::MutableCountersRef;
use repo_identity::RepoIdentityRef;

use super::Repo;
use super::LATEST_REPLAYED_REQUEST_KEY;

#[derive(Args)]
pub struct HgSyncRemainsArgs {
    /// Just print the number of entries
    #[clap(long, short = 'q')]
    quiet: bool,

    /// Exclude blobimport entries from the count
    #[clap(long)]
    without_blobimport: bool,
}

pub async fn remains(
    ctx: &CoreContext,
    repo: &Repo,
    remains_args: HgSyncRemainsArgs,
) -> Result<()> {
    let repo_name = repo.repo_identity().name();
    let repo_id = repo.repo_identity().id();

    // This is not quite correct: the counter not existing and having the
    // value 0 are actually different, but this is an edge case we will ignore.
    let counter = repo
        .mutable_counters()
        .get_counter(ctx, LATEST_REPLAYED_REQUEST_KEY)
        .await?
        .unwrap_or(0)
        .try_into()?;

    let exclude_reason = remains_args
        .without_blobimport
        .then(|| BookmarkUpdateReason::Blobimport);

    let remaining = repo
        .bookmark_update_log()
        .count_further_bookmark_log_entries(ctx.clone(), counter, exclude_reason)
        .await
        .with_context(|| {
            format!(
                "Failed to fetch remaining bundles to replay for {} ({})",
                repo_name, repo_id
            )
        })?;

    if remains_args.quiet {
        println!("{}", remaining);
    } else {
        let kind = if remains_args.without_blobimport {
            "non-blobimport bundles"
        } else {
            "bundles"
        };

        println!(
            "Remaining {} to replay in {} ({}): {}",
            kind, repo_name, repo_id, remaining
        );
    }

    Ok(())
}
