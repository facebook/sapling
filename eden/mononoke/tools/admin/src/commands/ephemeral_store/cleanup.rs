/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use anyhow::bail;
use clap::Args;
use cloned::cloned;
use context::CoreContext;
use futures::StreamExt;
use futures::stream;
use mononoke_macros::mononoke;

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

    /// The concurrency with which the bubbles will be cleaned up. Defaults to 100.
    #[clap(long, short = 'j', default_value_t = 100)]
    concurrency: usize,
}

pub async fn clean_bubbles(
    ctx: &CoreContext,
    repo: &Repo,
    args: EphemeralStoreCleanUpArgs,
) -> Result<()> {
    let cutoff_duration = Duration::from_secs(args.cutoff.into());
    let expired_bubbles = repo
        .repo_ephemeral_store
        .get_expired_bubbles(ctx, cutoff_duration, args.limit)
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
        let total = expired_bubbles.len();
        let finished = Arc::new(AtomicUsize::new(0));
        let already_deleted = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicUsize::new(0));
        let started = Instant::now();
        let ctx = ctx.clone();
        let store = repo.repo_ephemeral_store.clone();
        let result = mononoke::spawn_task({
            cloned!(finished, already_deleted, failed);
            async move {
                stream::iter(expired_bubbles)
                    .for_each_concurrent(args.concurrency, |id| {
                        cloned!(ctx, store, finished, already_deleted, failed);
                        async move {
                            match store.delete_bubble(&ctx, id).await {
                                Ok(outcome) => {
                                    finished.fetch_add(1, Ordering::Relaxed);
                                    if outcome.keys_already_deleted > 0 {
                                        already_deleted.fetch_add(1, Ordering::Relaxed);
                                    }
                                    println!(
                                        "Cleaned up bubble {id} and deleted {} blob keys contained in it",
                                        outcome.keys_deleted
                                    );
                                }
                                Err(err) => {
                                    failed.fetch_add(1, Ordering::Relaxed);
                                    eprintln!("Failed to clean up bubble {id}: {err:#}");
                                }
                            }
                        }
                    })
                    .await
            }
        })
        .await;
        let failed = failed.load(Ordering::Relaxed);
        println!(
            "Deleted {} of {total} expired bubbles in {:.1?}, {} of them had blob keys that were already deleted, {failed} failed",
            finished.load(Ordering::Relaxed),
            started.elapsed(),
            already_deleted.load(Ordering::Relaxed)
        );
        result?;
        if failed > 0 {
            bail!("Failed to clean up {failed} of {total} expired bubbles");
        }
    } else {
        println!(
            "Executing cleanup in dry-run mode. The following bubbles were fetched for deletion:"
        );
        println!("{expired_bubbles:?}");
    }
    Ok(())
}
