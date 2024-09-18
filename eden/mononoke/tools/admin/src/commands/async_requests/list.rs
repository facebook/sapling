/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_requests::types::ThriftAsynchronousRequestParams;
use clap::Args;
use client::AsyncRequestsQueue;
use context::CoreContext;
use mononoke_api::MononokeRepo;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::Timestamp;
use prettytable::format;
use prettytable::row;
use prettytable::Table;

#[derive(Args)]
/// Lists asynchronous requests (by default the ones active
/// now or updated within last 5 mins).
pub struct AsyncRequestsListArgs {
    /// Limits the results to the requests updated
    /// in the last N seconds.
    #[clap(long, default_value = "3600")]
    lookback: i64,
}

pub async fn list_requests(
    args: AsyncRequestsListArgs,
    ctx: CoreContext,
    queues_client: AsyncRequestsQueue,
) -> Result<(), Error> {
    let queue = queues_client
        .async_method_request_queue(&ctx)
        .await
        .context("obtaining async queue")?;

    let lookback = args.lookback;
    let mut table = Table::new();
    table.set_titles(row![
        "Request id",
        "Method",
        "Status",
        "Target bookmark",
        "Source name (sync_changeset)",
        "Source Changeset (sync_changeset)",
        "Created at",
        "Ready at",
        "Duration",
    ]);
    let res = queue
        .list_requests(
            &ctx,
            Some(&Timestamp::from_timestamp_secs(
                Timestamp::now().timestamp_seconds() - lookback,
            )),
            false,
        )
        .await
        .context("listing queued requests")?;
    for (req_id, entry, params) in res.into_iter() {
        let (source_name, changeset_id) = match params.thrift() {
            ThriftAsynchronousRequestParams::megarepo_sync_changeset_params(params) => (
                params.source_name.clone(),
                ChangesetId::from_bytes(params.cs_id.clone())
                    .context("deserializing entry")?
                    .to_string(),
            ),
            _ => ("".to_string(), "".to_string()),
        };
        let created_at: DateTime = entry.created_at.into();
        let ready_at: Option<DateTime> = entry.ready_at.map(|t| t.into());
        let ready_at_str = ready_at.map_or_else(|| "Not finished".to_string(), |t| t.to_string());
        let duration = if let Some(ready_at) = ready_at {
            let duration = ready_at.into_chrono() - created_at.into_chrono();
            duration.to_string()
        } else {
            "Not finished".to_string()
        };
        table.add_row(row![
            req_id.0,                                             // Request id
            req_id.1,                                             // Method
            entry.status,                                         // Status
            params.target().context("decoding target")?.bookmark, // Bookmark
            &source_name,                                         // Source name
            &changeset_id,                                        // Source Changeset
            &created_at,                                          // Created at
            &ready_at_str,                                        // Ready at
            duration,                                             // Duration
        ]);
    }

    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.printstd();

    Ok(())
}
