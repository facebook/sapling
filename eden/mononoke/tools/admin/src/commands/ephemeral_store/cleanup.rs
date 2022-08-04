/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use futures::future::try_join_all;
use futures::TryFutureExt;

use super::Repo;

// By default, at most 50 expired bubbles will be cleaned up in one go.
const DEFAULT_MAX_BUBBLES_FOR_CLEANUP: u32 = 50;
const DEFAULT_CUTOFF_FOR_CLEANUP: u32 = 24 * 60 * 60;

#[derive(Args)]
/// Subcommand to cleanup expired bubbles within the ephemeral store.
pub struct EphemeralStoreCleanUpArgs {
    /// Duration in seconds for which the bubbles should already
    /// be expired. Defaults to the number of seconds in a day.
    #[clap(long, short = 'c', default_value_t = DEFAULT_CUTOFF_FOR_CLEANUP)]
    cutoff: u32,

    /// The maximum number of bubbles that can be cleaned up in
    /// one run of the command.
    #[clap(long, short = 'l', default_value_t = DEFAULT_MAX_BUBBLES_FOR_CLEANUP)]
    limit: u32,

    /// When set, the command won't actually cleanup the bubbles but
    /// instead just lists the bubble IDs that will be cleaned-up on
    /// a non-dryrun of this command.
    #[clap(long, short = 'n')]
    dryrun: bool,
}

pub async fn clean_bubbles(
    ctx: &CoreContext,
    repo: &Repo,
    args: EphemeralStoreCleanUpArgs,
) -> Result<()> {
    let cutoff_duration = Duration::from_secs(args.cutoff.into());
    let expired_bubbles = repo
        .repo_ephemeral_store
        .get_expired_bubbles(cutoff_duration, args.limit)
        .await?;
    if expired_bubbles.is_empty() {
        println!("No expired bubbles found for deletion based on input provided");
        return Ok(());
    } else {
        println!(
            "Fetched {} expired bubbles for deletion",
            expired_bubbles.len()
        );
    }
    if !args.dryrun {
        let delete_futures = expired_bubbles.iter().map(|id| {
            repo.repo_ephemeral_store
                .delete_bubble(*id, ctx)
                .map_ok(|count| (id.clone(), count))
        });
        let bubbleid_and_count = try_join_all(delete_futures).await?;
        for (id, count) in bubbleid_and_count.iter() {
            println!(
                "Cleaned up bubble {} and deleted {} blob keys contained in it",
                id, count
            );
        }
    } else {
        println!(
            "Executing cleanup in dry-run mode. The following bubbles were fetched for deletion:"
        );
        println!("{:?}", expired_bubbles);
    }
    Ok(())
}
