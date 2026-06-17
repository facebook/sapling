/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use async_requests::AsyncMethodRequestQueue;
use async_requests::RowId;
use clap::Args;
use context::CoreContext;
use mononoke_types::DateTime;
use prettytable::Table;
use prettytable::format;
use prettytable::row;

#[derive(Args)]
/// Marks "dead" `ready` requests as `failed`: `ready` requests whose serialized
/// params blob is missing from the blobstore. Such requests can never be
/// processed or shown (`show` on them fails with "Missing blob"), so they are
/// transitioned to the `failed` state in the queue.
///
/// Only `ready` requests are scanned: in-flight (`new`/`inprogress`) requests
/// have just had their params written and are not interesting, and skipping
/// them keeps the scan (one blobstore lookup per request) fast. The scan is
/// keyset-paginated over the primary key. Use `--dry-run` to report what would
/// change without modifying anything.
pub struct AsyncRequestsFailDeadReadyRequestsArgs {
    /// Only report the dead requests; do not change their status.
    #[clap(long)]
    dry_run: bool,
    /// Number of requests to read from the DB per batch (one DB query each).
    #[clap(long, default_value = "10000")]
    batch_size: usize,
    /// Maximum number of concurrent blobstore presence checks per batch.
    #[clap(long, default_value = "100")]
    concurrency: usize,
}

pub async fn fail_dead_ready_requests(
    args: AsyncRequestsFailDeadReadyRequestsArgs,
    ctx: CoreContext,
    queue: AsyncMethodRequestQueue,
) -> Result<()> {
    let mut after_id: Option<RowId> = None;
    let mut total_scanned: usize = 0;
    let mut total_marked: u64 = 0;
    let mut dead = Vec::new();

    loop {
        let batch = queue
            .list_orphan_requests(&ctx, after_id, args.batch_size, args.concurrency)
            .await
            .context("scanning for dead ready requests")?;
        total_scanned += batch.scanned;

        if !args.dry_run && !batch.orphans.is_empty() {
            let ids: Vec<RowId> = batch.orphans.iter().map(|entry| entry.id.clone()).collect();
            total_marked += queue
                .mark_requests_failed(&ctx, &ids)
                .await
                .context("marking dead ready requests as failed")?;
        }
        dead.extend(batch.orphans);

        // Advance the cursor only if the batch was full; a short batch means we
        // have reached the end of the ready requests.
        match batch.last_scanned_id {
            Some(id) if batch.scanned == args.batch_size => after_id = Some(id),
            _ => break,
        }
    }

    // Sort by request id to stabilise output.
    dead.sort_by_key(|entry| entry.id.0);

    let mut table = Table::new();
    table.set_titles(row![
        "Request id",
        "Method",
        "Repo id",
        "Status",
        "Created at",
        "Args blobstore key",
    ]);
    for entry in &dead {
        let created_at: DateTime = entry.created_at.into();
        let repo_id = entry
            .repo_id
            .map_or_else(|| "(none)".to_string(), |id| id.id().to_string());
        table.add_row(row![
            entry.id.0,                  // Request id
            &entry.request_type,         // Method
            &repo_id,                    // Repo id
            &entry.status,               // Status
            &created_at,                 // Created at
            &entry.args_blobstore_key.0  // Args blobstore key
        ]);
    }
    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.printstd();

    if args.dry_run {
        println!(
            "\nDry run: found {} dead ready request(s) out of {} scanned; none were modified.",
            dead.len(),
            total_scanned,
        );
    } else {
        println!(
            "\nMarked {} dead ready request(s) as failed (found {}, scanned {}).",
            total_marked,
            dead.len(),
            total_scanned,
        );
    }

    Ok(())
}
