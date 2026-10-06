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

/// Default threshold matching the `queue.<repo>.age_s.ready` alerts
/// (e.g. "async_requests worker has old requests ready for the fbsource
/// repo"): 91 days in seconds.
const DEFAULT_OLDER_THAN_SECS: i64 = 7862400;

#[derive(Args)]
/// Lists `ready` async requests older than a threshold: the requests that
/// would trip the `queue.<repo>.age_s.ready` worker-stat alerts.
///
/// The worker stats loop reports `now - min(ready_at)` per repo and status,
/// so any `ready` request with `ready_at` older than `now - threshold`
/// keeps that stat above the threshold. Backfill request types are excluded
/// by default to match the stats loop.
pub struct AsyncRequestsListOldReadyArgs {
    /// The repository name or ID to filter by (all repos if omitted).
    #[clap(flatten)]
    pub repo: OptRepoArgs,
    /// Only show requests with `ready_at` older than this many seconds ago.
    /// Must be non-negative: a negative value would push the cutoff into the
    /// future and match every `ready` request.
    #[clap(
        long,
        default_value_t = DEFAULT_OLDER_THAN_SECS,
        value_parser = clap::value_parser!(i64).range(0..)
    )]
    older_than_secs: i64,
    /// Maximum number of requests to show (oldest first).
    #[clap(long, default_value_t = 100)]
    limit: usize,
    /// Include derived data backfill request types (excluded by default).
    #[clap(long)]
    include_backfill: bool,
}

pub async fn list_old_ready_requests(
    args: AsyncRequestsListOldReadyArgs,
    ctx: CoreContext,
    queue: AsyncMethodRequestQueue,
) -> Result<()> {
    let now_secs = Timestamp::now().timestamp_seconds();
    let ready_before = Timestamp::from_timestamp_secs(now_secs - args.older_than_secs);
    let entries = queue
        .list_old_ready_requests(&ctx, &ready_before, args.limit, !args.include_backfill)
        .await?;

    let mut table = Table::new();
    table.set_titles(row![
        "Request id",
        "Method",
        "Repo id",
        "Created at",
        "Ready at",
        "Age (s)",
        "Args blobstore key",
    ]);
    for entry in &entries {
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
            entry.id.0,                  // Request id
            &entry.request_type.0,       // Method
            &repo_id,                    // Repo id
            &created_at,                 // Created at
            &ready_at_str,               // Ready at
            &age_s,                      // Age (s)
            &entry.args_blobstore_key.0  // Args blobstore key
        ]);
    }
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.printstd();

    let ready_before_dt: DateTime = ready_before.into();
    if entries.is_empty() {
        println!(
            "\nNo ready requests older than {}s (ready_at < {}).",
            args.older_than_secs, ready_before_dt,
        );
    } else {
        println!(
            "\nFound {} ready request(s) older than {}s (ready_at < {}), oldest first.",
            entries.len(),
            args.older_than_secs,
            ready_before_dt,
        );
    }

    Ok(())
}
