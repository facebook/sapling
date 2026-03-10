/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod display;
mod types;

use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Args;
use context::CoreContext;
use mononoke_types::Timestamp;
use requests_table::LongRunningRequestsQueue;
use requests_table::RequestStatus;
use requests_table::RowId;
use requests_table::SqlLongRunningRequestsQueue;

use self::display::display_backfill_list;
use self::display::display_single_repo_detail;
use self::types::BackfillDisplayData;

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
    queue: SqlLongRunningRequestsQueue,
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
    ctx: &CoreContext,
    queue: &impl LongRunningRequestsQueue,
    lookback_days: i64,
) -> Result<()> {
    let now = Timestamp::now();
    let lookback_seconds = lookback_days * 24 * 60 * 60;
    let min_created_at = Timestamp::from_timestamp_secs(now.timestamp_seconds() - lookback_seconds);

    let backfills = queue
        .list_recent_backfills_with_repo_count(ctx, &min_created_at)
        .await
        .context("fetching recent backfills")?;

    if backfills.is_empty() {
        println!("No backfills found in the last {} days", lookback_days);
        return Ok(());
    }

    display_backfill_list(backfills);

    Ok(())
}

async fn show_backfill_detail(
    ctx: &CoreContext,
    queue: &impl LongRunningRequestsQueue,
    row_id: &RowId,
) -> Result<()> {
    // Step 1: Verify the backfill exists
    let root_entry = queue
        .get_backfill_root_entry(ctx, row_id)
        .await
        .context("fetching backfill root entry")?;

    let (request_id, request_type, status, created_at, _args_key) = match root_entry {
        Some(entry) => entry,
        None => bail!(
            "Invalid request ID: {} is not a backfill root request",
            row_id.0
        ),
    };

    // Step 2: Get aggregated stats
    let stats_by_status = queue
        .get_backfill_stats(ctx, row_id, None)
        .await
        .context("fetching stats by status")?;

    if stats_by_status.is_empty() {
        println!(
            "Backfill {} not yet started (waiting for worker to process)",
            request_id.0
        );
        return Ok(());
    }

    // Step 3: Get timing stats
    let (completed_count, avg_duration_secs, _min_created_at, _max_ready_at) = queue
        .get_backfill_timing_stats(ctx, row_id)
        .await
        .context("fetching timing stats")?;

    // Step 4: Calculate metrics
    let now = Timestamp::now();
    let elapsed_time =
        Duration::from_secs((now.timestamp_seconds() - created_at.timestamp_seconds()) as u64);

    let avg_duration = avg_duration_secs.map(Duration::from_secs_f64);

    let elapsed_hours = elapsed_time.as_secs_f64() / 3600.0;
    let requests_per_hour = if elapsed_hours > 0.0 {
        completed_count as f64 / elapsed_hours
    } else {
        0.0
    };

    // Count by status
    let mut status_map: HashMap<RequestStatus, usize> = HashMap::new();
    let mut total_requests = 0;
    for (_req_type, req_status, count) in &stats_by_status {
        *status_map.entry(*req_status).or_insert(0) += *count as usize;
        total_requests += *count as usize;
    }

    let new_count = *status_map.get(&RequestStatus::New).unwrap_or(&0);
    let inprogress_count = *status_map.get(&RequestStatus::InProgress).unwrap_or(&0);
    let pending_count = new_count + inprogress_count;

    let estimated_remaining = if requests_per_hour > 0.0 && elapsed_time.as_secs() > 300 {
        Some(Duration::from_secs_f64(
            (pending_count as f64 / requests_per_hour) * 3600.0,
        ))
    } else {
        None
    };

    // Convert status_map to sorted vec
    let mut status_counts: Vec<(RequestStatus, usize)> = status_map.into_iter().collect();
    status_counts.sort_by(|a, b| b.1.cmp(&a.1));

    // Group by request type
    let mut type_map: HashMap<String, Vec<(RequestStatus, usize)>> = HashMap::new();
    for (req_type, req_status, count) in &stats_by_status {
        type_map
            .entry(req_type.0.clone())
            .or_insert_with(Vec::new)
            .push((req_status.clone(), *count as usize));
    }
    let mut type_breakdown: Vec<(String, Vec<(RequestStatus, usize)>)> =
        type_map.into_iter().collect();
    type_breakdown.sort_by(|a, b| a.0.cmp(&b.0));

    let data = BackfillDisplayData {
        request_id,
        created_at,
        status,
        request_type: request_type.to_string(),
        total_requests,
        status_counts,
        type_breakdown,
        elapsed_time,
        avg_duration,
        requests_per_hour,
        estimated_remaining,
    };

    // Check if this is a single-repo backfill
    let stats_by_repo = queue
        .get_backfill_stats_by_repo(ctx, row_id)
        .await
        .context("fetching stats by repo")?;

    let unique_repos: HashSet<_> = stats_by_repo
        .iter()
        .filter_map(|(repo_id, _, _)| *repo_id)
        .collect();
    let is_single_repo = unique_repos.len() <= 1;

    if is_single_repo {
        let repo_id = unique_repos.iter().next().map(|r| r.id() as i64);
        display_single_repo_detail(&data, repo_id);
    } else {
        // Multi-repo backfill: show condensed view (to be implemented in next commit)
        println!("Multi-repo backfill view - to be implemented");
    }

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
