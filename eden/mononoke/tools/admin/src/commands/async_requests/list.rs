/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_requests::AsyncMethodRequestQueue;
use async_requests::types::ThriftAsynchronousRequestParams;
use clap::Args;
use context::CoreContext;
use mononoke_api::MononokeRepo;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::Timestamp;
use prettytable::Table;
use prettytable::format;
use prettytable::row;

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
    queue: AsyncMethodRequestQueue,
) -> Result<(), Error> {
    let lookback = args.lookback;
    let mut table = Table::new();
    table.set_titles(row![
        "Request id",
        "Method",
        "Status",
        "Target",
        "Source name (sync_changeset)",
        "Source Changeset (sync_changeset)",
        "Created at",
        "Ready at",
        "Duration",
    ]);
    let mut res = queue
        .list_requests(
            &ctx,
            Some(&Timestamp::from_timestamp_secs(
                Timestamp::now().timestamp_seconds() - lookback,
            )),
            false,
        )
        .await
        .context("listing queued requests")?;
    // sort by request id to stabilise output
    res.sort_by_key(|(req_id, _, _)| req_id.0.0);
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
        let target_str = match params.target() {
            Ok(target) => target.to_string(),
            Err(_) => "(failed to convert)".to_string(),
        };
        table.add_row(row![
            req_id.0,      // Request id
            req_id.1,      // Method
            entry.status,  // Status
            target_str,    // Target
            &source_name,  // Source name
            &changeset_id, // Source Changeset
            &created_at,   // Created at
            &ready_at_str, // Ready at
            duration,      // Duration
        ]);
    }

    table.set_format(*format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.printstd();

    Ok(())
}
