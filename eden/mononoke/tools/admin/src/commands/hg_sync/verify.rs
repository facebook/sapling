/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
pub struct HgSyncVerifyArgs {}

pub async fn verify(ctx: &CoreContext, repo: &Repo, _verify_args: HgSyncVerifyArgs) -> Result<()> {
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

    let counts = repo
        .bookmark_update_log()
        .count_further_bookmark_log_entries_by_reason(ctx.clone(), counter)
        .await?;

    let mut blobimports = 0;
    let mut others = 0;
    for (reason, count) in counts {
        if reason == BookmarkUpdateReason::Blobimport {
            blobimports += count;
        } else {
            others += count;
        }
    }

    match (blobimports > 0, others > 0) {
        (true, true) => {
            println!(
                "Remaining bundles to replay in {} ({}) are not consistent: found {} blobimports and {} non-blobimports",
                repo_name, repo_id, blobimports, others
            );
        }
        (true, false) => {
            println!(
                "All remaining bundles in {} ({}) are blobimports (found {})",
                repo_name, repo_id, blobimports,
            );
        }
        (false, true) => {
            println!(
                "All remaining bundles in {} ({}) are non-blobimports (found {})",
                repo_name, repo_id, others,
            );
        }
        (false, false) => {
            println!("No replay data found in {} ({})", repo_name, repo_id);
        }
    }

    Ok(())
}
