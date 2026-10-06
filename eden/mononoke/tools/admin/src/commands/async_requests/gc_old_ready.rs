/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_requests::AsyncMethodRequestQueue;
use clap::Args;
use context::CoreContext;
use mononoke_app::args::OptRepoArgs;
use mononoke_types::DateTime;
use mononoke_types::Timestamp;
use prettytable::Table;
use prettytable::format;
use prettytable::row;
use requests_table::LongRunningRequestEntry;
use requests_table::RowId;

/// Default threshold matching the `queue.<repo>.age_s.ready` alerts
/// (e.g. "async_requests worker has old requests ready for the fbsource
/// repo"): 91 days in seconds.
const DEFAULT_OLDER_THAN_SECS: i64 = 7862400;

/// Request types safe to garbage-collect once orphaned: the client either
/// never needed the result, or the result is stale by the time we reap it,
/// and marking the row `polled` preserves the result blob so a late poller
/// still gets its response.
///
/// - `commit_sparse_profile_size_async`: the sparse-size tailer discards the
///   result even on success (`recalculate_total_size` returns `None`).
/// - `commit_sparse_profile_delta_async`: the tailer consumes the delta only
///   in its incremental path; an orphaned delta was never consumed, and by
///   the time it is old enough to reap, the sizes it describes are stale.
const BENIGN_REQUEST_TYPES: &[&str] = &[
    "commit_sparse_profile_size_async",
    "commit_sparse_profile_delta_async",
];

#[derive(Args)]
/// Garbage-collects orphaned `ready` async requests of known-benign types by
/// marking them `polled`, as if the client had collected their results.
///
/// These are the requests that would trip the `queue.<repo>.age_s.ready`
/// worker-stat alerts (the stat reports `now - min(ready_at)`). Only the
/// hardcoded benign types are ever touched; anything else is reported but
/// left alone. Rows without a stored result are skipped (a late poll of those
/// would fail), as is anything that left the `ready` state since listing.
/// Use `--dry-run` to report what would change without modifying anything.
pub struct AsyncRequestsGcOldReadyArgs {
    /// The repository name or ID to filter by (all repos if omitted).
    #[clap(flatten)]
    pub repo: OptRepoArgs,
    /// Only collect requests with `ready_at` older than this many seconds ago.
    /// Must be non-negative: a negative value would push the cutoff into the
    /// future and match every `ready` request.
    #[clap(
        long,
        default_value_t = DEFAULT_OLDER_THAN_SECS,
        value_parser = clap::value_parser!(i64).range(0..)
    )]
    older_than_secs: i64,
    /// Maximum number of requests to list (oldest first).
    #[clap(long, default_value_t = 1000)]
    limit: usize,
    /// Only report what would be collected; do not change anything.
    #[clap(long)]
    dry_run: bool,
}

pub async fn gc_old_ready_requests(
    args: AsyncRequestsGcOldReadyArgs,
    ctx: CoreContext,
    queue: AsyncMethodRequestQueue,
) -> Result<()> {
    let now_secs = Timestamp::now().timestamp_seconds();
    let ready_before = Timestamp::from_timestamp_secs(now_secs - args.older_than_secs);
    let entries = queue
        .list_old_ready_requests(&ctx, &ready_before, args.limit, true)
        .await?;

    let (benign, other): (Vec<&_>, Vec<&_>) = entries
        .iter()
        .partition(|entry| BENIGN_REQUEST_TYPES.contains(&entry.request_type.0.as_str()));
    let (flippable, missing_result): (Vec<&_>, Vec<&_>) = benign
        .into_iter()
        .partition(|entry| entry.result_blobstore_key.is_some());

    let marked: u64 = if !args.dry_run && !flippable.is_empty() {
        let ids: Vec<RowId> = flippable.iter().map(|entry| entry.id.clone()).collect();
        queue.mark_requests_polled(&ctx, &ids).await?
    } else {
        0
    };

    print_table(
        "Collectable (benign type, result stored)",
        &flippable,
        now_secs,
    );
    print_table("Skipped (no stored result)", &missing_result, now_secs);
    print_table("Skipped (not a benign type)", &other, now_secs);

    if entries.len() == args.limit {
        println!(
            "\nNote: listing hit the --limit of {}; re-run to see whether more rows match.",
            args.limit
        );
    }
    let ready_before_dt: DateTime = ready_before.into();
    if args.dry_run {
        println!(
            "\nDry run: would mark {} request(s) as polled out of {} matching (ready_at < {}); none were modified.",
            flippable.len(),
            entries.len(),
            ready_before_dt,
        );
    } else {
        println!(
            "\nMarked {} request(s) as polled ({} flippable out of {} matching, ready_at < {}).",
            marked,
            flippable.len(),
            entries.len(),
            ready_before_dt,
        );
    }

    Ok(())
}

fn print_table(title: &str, entries: &[&LongRunningRequestEntry], now_secs: i64) {
    println!("\n{title}: {}", entries.len());
    if entries.is_empty() {
        return;
    }
    let mut table = Table::new();
    table.set_titles(row![
        "Request id",
        "Method",
        "Repo id",
        "Created at",
        "Ready at",
        "Age (s)",
    ]);
    for entry in entries {
        let created_at: DateTime = entry.created_at.into();
        let (ready_at_str, age_s) = entry.ready_at.map_or_else(
            || ("(none)".to_string(), "(none)".to_string()),
            |ready_at| {
                let age = now_secs - ready_at.timestamp_seconds();
                (DateTime::from(ready_at).to_string(), age.to_string())
            },
        );
        let repo_id = entry
            .repo_id
            .map_or_else(|| "(none)".to_string(), |id| id.id().to_string());
        table.add_row(row![
            entry.id.0,            // Request id
            &entry.request_type.0, // Method
            &repo_id,              // Repo id
            &created_at,           // Created at
            &ready_at_str,         // Ready at
            &age_s,                // Age (s)
        ]);
    }
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.printstd();
}
