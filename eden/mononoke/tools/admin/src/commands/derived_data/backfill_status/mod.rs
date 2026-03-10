/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod display;
mod types;

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use requests_table::LongRunningRequestsQueue;
use requests_table::RowId;

#[derive(Args)]
pub(super) struct BackfillStatusArgs {
    /// Request ID of the backfill to examine.
    /// If omitted, lists all recent backfills.
    #[clap(long)]
    request_id: Option<u64>,

    /// For a multi-repo backfill, drill down on a specific repo
    #[clap(long)]
    repo_id: Option<i64>,

    /// Lookback window in days for listing backfills
    #[clap(long, default_value = "7")]
    lookback: i64,
}

pub(super) async fn backfill_status(
    ctx: &CoreContext,
    queue: impl LongRunningRequestsQueue,
    args: BackfillStatusArgs,
) -> Result<()> {
    match args.request_id {
        None => {
            // Mode 1: List recent backfills
            list_backfills(ctx, &queue, args.lookback).await?;
        }
        Some(request_id) => {
            // Mode 2: Show detailed progress for a specific backfill
            let row_id = RowId(request_id);
            match args.repo_id {
                None => {
                    // Show overall backfill progress
                    show_backfill_detail(ctx, &queue, &row_id).await?;
                }
                Some(repo_id) => {
                    // Drill down into a specific repo
                    show_repo_detail(ctx, &queue, &row_id, repo_id).await?;
                }
            }
        }
    }

    Ok(())
}

async fn list_backfills(
    _ctx: &CoreContext,
    _queue: &impl LongRunningRequestsQueue,
    _lookback_days: i64,
) -> Result<()> {
    println!("Backfill list view - to be implemented");
    Ok(())
}

async fn show_backfill_detail(
    _ctx: &CoreContext,
    _queue: &impl LongRunningRequestsQueue,
    _row_id: &RowId,
) -> Result<()> {
    // To be implemented in the next commit
    println!("Backfill detail view - to be implemented");
    Ok(())
}

async fn show_repo_detail(
    _ctx: &CoreContext,
    _queue: &impl LongRunningRequestsQueue,
    _row_id: &RowId,
    _repo_id: i64,
) -> Result<()> {
    // To be implemented in the next commit
    println!("Repo detail view - to be implemented");
    Ok(())
}
