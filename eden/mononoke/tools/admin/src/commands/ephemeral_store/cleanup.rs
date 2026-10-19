/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Reverse;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use clap::Args;
use cloned::cloned;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use itertools::Itertools;
use mononoke_macros::mononoke;
use tokio::time::sleep;
use tokio_util::task::AbortOnDropHandle;

use super::Repo;

// By default, at most 50 expired bubbles will be cleaned up in one go.
const DEFAULT_MAX_BUBBLES_FOR_CLEANUP: u32 = 50;
const DEFAULT_CUTOFF_FOR_CLEANUP: u32 = 24 * 60 * 60;
const PROGRESS_REPORT_INTERVAL: Duration = Duration::from_secs(10 * 60);
const THREAD_STATES_REPORT_INTERVAL: Duration = Duration::from_secs(15 * 60);

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

/// Runs `report` every `interval` in a separate task, until the returned
/// handle is dropped.
fn report_periodically(
    interval: Duration,
    report: impl Fn() + Send + 'static,
) -> AbortOnDropHandle<()> {
    AbortOnDropHandle::new(mononoke::spawn_task(async move {
        loop {
            sleep(interval).await;
            report();
        }
    }))
}

/// Logs how many threads share each name, scheduler state and kernel wait channel.
fn log_thread_states() {
    let entries = match fs::read_dir("/proc/self/task") {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("Cannot read thread states: {e}");
            return;
        }
    };
    eprintln!("Thread states (count, name, state, wait channel):");
    entries
        .filter_map(|entry| thread_state(&entry.ok()?.path()))
        .counts()
        .into_iter()
        .sorted_by_key(|(_, count)| Reverse(*count))
        .for_each(|((name, state, wchan), count)| eprintln!("  {count} {name} {state} {wchan}"));
}

fn thread_state(task_dir: &Path) -> Option<(String, char, String)> {
    let name = fs::read_to_string(task_dir.join("comm")).ok()?;
    let stat = fs::read_to_string(task_dir.join("stat")).ok()?;
    let state = stat.rsplit_once(')')?.1.trim_start().chars().next()?;
    let wchan = fs::read_to_string(task_dir.join("wchan")).unwrap_or_default();
    Some((
        name.trim_end()
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .to_owned(),
        state,
        wchan,
    ))
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
        let _progress_report = report_periodically(PROGRESS_REPORT_INTERVAL, {
            cloned!(finished);
            move || {
                let finished = finished.load(Ordering::Relaxed);
                eprintln!(
                    "Cleanup progress: {finished} bubbles cleaned up, {} left",
                    total - finished
                );
            }
        });
        let _thread_states_report = report_periodically(THREAD_STATES_REPORT_INTERVAL, || {
            mononoke::spawn_blocking(log_thread_states);
        });
        let ctx = ctx.clone();
        let store = repo.repo_ephemeral_store.clone();
        mononoke::spawn_task(async move {
            stream::iter(expired_bubbles)
                .map(Ok)
                .map_ok(|id| {
                    cloned!(ctx, store, finished);
                    async move {
                        let count = store.delete_bubble(&ctx, id).await?;
                        finished.fetch_add(1, Ordering::Relaxed);
                        println!(
                            "Cleaned up bubble {id} and deleted {count} blob keys contained in it"
                        );
                        anyhow::Ok(())
                    }
                })
                .try_buffer_unordered(args.concurrency)
                .try_collect::<Vec<_>>()
                .await
        })
        .await??;
    } else {
        println!(
            "Executing cleanup in dry-run mode. The following bubbles were fetched for deletion:"
        );
        println!("{expired_bubbles:?}");
    }
    Ok(())
}
