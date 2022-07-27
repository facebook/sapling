/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
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
pub struct HgSyncLastProcessedArgs {
    /// Set the last processed log entry to a specific id
    #[clap(long, value_name = "ID")]
    set: Option<i64>,

    /// Set the last processed log entry such that we skip over blobimport
    /// entries
    #[clap(long, conflicts_with = "set")]
    skip_blobimport: bool,

    /// When skipping over blobimport entries, don't update the last processed
    /// entry, just print what would happen
    #[clap(long, short = 'n', requires = "skip-blobimport")]
    dry_run: bool,
}

pub async fn last_processed(
    ctx: &CoreContext,
    repo: &Repo,
    last_processed_args: HgSyncLastProcessedArgs,
) -> Result<()> {
    let repo_name = repo.repo_identity().name();
    let repo_id = repo.repo_identity().id();

    let maybe_counter = repo
        .mutable_counters()
        .get_counter(ctx, LATEST_REPLAYED_REQUEST_KEY)
        .await?;

    if let Some(counter) = maybe_counter {
        println!(
            "Counter for {} ({}) has value {}",
            repo_name, repo_id, counter
        );
    } else {
        println!("No counter found for {} ({})", repo_name, repo_id);
    }

    if let Some(new_value) = last_processed_args.set {
        repo.mutable_counters()
            .set_counter(ctx, LATEST_REPLAYED_REQUEST_KEY, new_value, None)
            .await
            .with_context(|| {
                format!(
                    "Failed to set counter for {} ({}) to {}",
                    repo_name, repo_id, new_value
                )
            })?;

        println!(
            "Counter for {} ({}) set to {}",
            repo_name, repo_id, new_value
        );
    } else if last_processed_args.skip_blobimport {
        let old_value =
            maybe_counter.ok_or_else(|| anyhow!("Cannot update counter without a value"))?;
        let new_value = repo
            .bookmark_update_log()
            .skip_over_bookmark_log_entries_with_reason(
                ctx.clone(),
                old_value.try_into()?,
                BookmarkUpdateReason::Blobimport,
            )
            .await?
            .ok_or_else(|| anyhow!("No valid counter position to skip ahead to"))?;

        if last_processed_args.dry_run {
            println!(
                "Counter for {} ({}) would be updated to {}",
                repo_name, repo_id, new_value
            );
        } else {
            let success = repo
                .mutable_counters()
                .set_counter(
                    ctx,
                    LATEST_REPLAYED_REQUEST_KEY,
                    new_value.try_into()?,
                    Some(old_value),
                )
                .await?;
            if success {
                println!(
                    "Counter for {} ({}) was updated to {}",
                    repo_name, repo_id, new_value
                );
            } else {
                bail!("Update failed due to update conflict");
            }
        }
    }

    Ok(())
}
