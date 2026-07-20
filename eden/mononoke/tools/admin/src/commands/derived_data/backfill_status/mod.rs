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
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use async_requests::types::AsynchronousRequestParams;
use async_requests::types::AsynchronousRequestResult;
use async_requests::types::DeriveBoundaries;
use async_requests::types::DeriveSlice;
use async_requests::types::RequestTypeName;
use async_requests::types::ThriftAsynchronousRequestParams;
use async_requests::types::ThriftAsynchronousRequestResult;
use blobstore::Blobstore;
use bulk_derivation::BulkDerivation;
use clap::Args;
use context::CoreContext;
use derived_data_manager::DerivedDataManager;
// Tupperware log reading is FB-internal only; keep it out of OSS builds.
#[cfg(fbcode_build)]
use fbinit::FacebookInit;
use futures::stream;
use futures::stream::StreamExt;
use git_source_of_truth::GitSourceOfTruthConfig;
use git_source_of_truth::GitSourceOfTruthConfigRef;
#[cfg(fbcode_build)]
use log_reader::GetSessionRequest;
#[cfg(fbcode_build)]
use log_reader::LineContentFilter;
#[cfg(fbcode_build)]
use log_reader::LogDirection;
#[cfg(fbcode_build)]
use log_reader::LogFilter;
#[cfg(fbcode_build)]
use log_reader::ReadLogLinesRequest;
#[cfg(fbcode_build)]
use log_reader::SourceIdentity;
#[cfg(fbcode_build)]
use log_reader::TaskIdentity;
#[cfg(fbcode_build)]
use log_reader_srclients::make_LogReader_srclient;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use phases::Phases;
use phases::PhasesRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use requests_table::BlobstoreKey;
use requests_table::LongRunningRequestEntry;
use requests_table::LongRunningRequestsQueue;
use requests_table::RecentBackfillEntry;
use requests_table::RequestStatus;
use requests_table::RowId;
use requests_table::SqlLongRunningRequestsQueue;
use strum::IntoEnumIterator;
use tracing::warn;
#[cfg(fbcode_build)]
use tupperware_api_common::JobHandle;

use self::display::BackfillListRow;
use self::display::display_backfill_list;
use self::display::display_child_request_detail;
use self::display::display_child_request_table;
use self::display::display_multi_repo_summary;
use self::display::display_repo_detail;
use self::display::display_repo_detail_table;
use self::display::display_single_repo_detail;
#[cfg(fbcode_build)]
use self::display::format_timestamp;
use self::types::BackfillChildDisplayData;
use self::types::BackfillChildParams;
use self::types::BackfillChildResult;
use self::types::BackfillDisplayData;
use self::types::BackfillSettings;
use self::types::BoundaryDerivationStatus;
use self::types::ChildCounts;
use self::types::ChildRequestRow;
use self::types::RepoDetailRow;
use self::types::RepoDisplayData;
use self::types::RepoStatus;
use self::types::SliceSegmentDisplayData;
use super::Repo;

/// Lightweight repo container for querying phase counts without opening
/// the full derived-data `Repo`.
#[facet::container]
struct PhaseCountRepo {
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    phases: dyn Phases,
}

/// Lightweight repo container for reading the (global) git source-of-truth
/// config, used to resolve names for git repos that aren't in the loaded
/// repo configs.
#[facet::container]
struct GitSotRepo {
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    git_source_of_truth_config: dyn GitSourceOfTruthConfig,
}

/// How many backfills to load params for in parallel for the list view.
const PARAMS_LOAD_CONCURRENCY: usize = 16;

#[derive(Args)]
pub(super) struct BackfillStatusArgs {
    /// Request ID of the backfill to examine.
    /// If omitted, lists all recent backfills.
    #[clap(long)]
    request_id: Option<u64>,

    /// Lookback window in days for listing backfills
    #[clap(long, default_value = "7")]
    lookback: i64,

    /// List all backfills that still have pending or in-progress work,
    /// regardless of age. Ignores --lookback.
    #[clap(long)]
    active: bool,

    /// Show extra details:
    /// - root of a multi-repo backfill: a per-repository table
    /// - root of a single (large) repo backfill: a per-child-request table
    /// - an individual child request: the claiming worker's Tupperware logs
    ///   over that request's processing window
    #[clap(long)]
    detailed: bool,

    /// Max number of worker log lines to show (the tail) for an individual
    /// child request under --detailed. If more lines exist in the window, a
    /// `tw log` command to see them all is printed.
    #[clap(long, default_value_t = 100)]
    max_log_lines: usize,
}

pub(super) async fn backfill_status(
    ctx: &CoreContext,
    app: &MononokeApp,
    queue: SqlLongRunningRequestsQueue,
    blobstore: Arc<dyn Blobstore>,
    repo_names: HashMap<RepositoryId, String>,
    args: BackfillStatusArgs,
    repo: Option<&Repo>,
    manager: Option<&DerivedDataManager>,
) -> Result<()> {
    match args.request_id {
        None => {
            // Mode 1: List recent backfills
            list_backfills(
                ctx,
                app,
                &queue,
                &blobstore,
                &repo_names,
                args.lookback,
                args.active,
            )
            .await?;
        }
        Some(request_id) => {
            // Mode 2: Show detailed progress for a specific backfill
            let row_id = RowId(request_id);
            show_backfill_detail(
                ctx,
                app,
                &queue,
                &blobstore,
                &repo_names,
                &row_id,
                repo,
                manager,
                args.detailed,
                args.max_log_lines,
            )
            .await?;
        }
    }

    Ok(())
}

/// Load `DeriveBackfillParams` from blobstore and extract the derived data type.
/// Returns `None` if the blob can't be loaded or the params are not a backfill.
async fn load_derived_data_type(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    args_blobstore_key: &BlobstoreKey,
) -> Option<String> {
    let params = AsynchronousRequestParams::load_from_key(ctx, blobstore, &args_blobstore_key.0)
        .await
        .ok()?;
    match params.thrift() {
        ThriftAsynchronousRequestParams::derive_backfill_params(p) => {
            Some(p.derived_data_type.clone())
        }
        _ => None,
    }
}

/// Load the root `DeriveBackfillParams` blob and extract both the derived data
/// type and the settings the backfill was enqueued with. Returns `(None, None)`
/// if the blob can't be loaded or the params are not a backfill.
async fn load_backfill_params_info(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    args_blobstore_key: &BlobstoreKey,
) -> (Option<String>, Option<BackfillSettings>) {
    let Ok(params) =
        AsynchronousRequestParams::load_from_key(ctx, blobstore, &args_blobstore_key.0).await
    else {
        return (None, None);
    };
    match params.thrift() {
        ThriftAsynchronousRequestParams::derive_backfill_params(p) => (
            Some(p.derived_data_type.clone()),
            Some(BackfillSettings {
                slice_size: p.slice_size,
                boundaries_concurrency: p.boundaries_concurrency,
                num_boundary_requests: p.num_boundary_requests,
                reslice: p.reslice,
                config_name: p.config_name.clone(),
            }),
        ),
        _ => (None, None),
    }
}

/// Whether a backfill still has pending or in-progress work: either its root
/// request hasn't finished spawning children, or some child request is still
/// queued or running.
fn backfill_has_active_work(entry: &RecentBackfillEntry) -> bool {
    matches!(
        entry.root_status,
        RequestStatus::New | RequestStatus::InProgress
    ) || entry.child_new_count > 0
        || entry.child_inprogress_count > 0
}

async fn list_backfills(
    ctx: &CoreContext,
    app: &MononokeApp,
    queue: &impl LongRunningRequestsQueue,
    blobstore: &Arc<dyn Blobstore>,
    repo_names: &HashMap<RepositoryId, String>,
    lookback_days: i64,
    all_active: bool,
) -> Result<()> {
    // With --all-active we drop the lookback window (query from the epoch) and
    // instead keep only backfills that still have active work, so long-running
    // backfills created before the window are still surfaced.
    let min_created_at = if all_active {
        Timestamp::from_timestamp_secs(0)
    } else {
        let now = Timestamp::now();
        let lookback_seconds = lookback_days * 24 * 60 * 60;
        Timestamp::from_timestamp_secs(now.timestamp_seconds() - lookback_seconds)
    };

    let backfills = queue
        .list_recent_backfills_with_repo_count(ctx, &min_created_at)
        .await
        .context("fetching recent backfills")?;

    let backfills: Vec<RecentBackfillEntry> = if all_active {
        backfills
            .into_iter()
            .filter(backfill_has_active_work)
            .collect()
    } else {
        backfills
    };

    if backfills.is_empty() {
        if all_active {
            println!("No active backfills found");
        } else {
            println!("No backfills found in the last {lookback_days} days");
        }
        return Ok(());
    }

    let rows: Vec<BackfillListRow> = stream::iter(backfills.into_iter().map(|entry| async move {
        let derived_data_type =
            load_derived_data_type(ctx, blobstore, &entry.args_blobstore_key).await;
        let children = ChildCounts {
            new: entry.child_new_count.max(0) as u64,
            inprogress: entry.child_inprogress_count.max(0) as u64,
            ready: entry.child_ready_count.max(0) as u64,
            failed: entry.child_failed_count.max(0) as u64,
        };
        let has_failed_requests = entry.root_status == RequestStatus::Failed || children.failed > 0;
        let has_active_work = backfill_has_active_work(&entry);
        let aggregate_status = if has_failed_requests && has_active_work {
            RepoStatus::InProgress
        } else {
            RepoStatus::from_root_and_children(entry.root_status, children)
        };
        // The list query only gives a repo count; fetch the distinct repo ids so
        // we can show repo names in the table.
        let repo_ids = match queue.get_backfill_stats_by_repo(ctx, &entry.id).await {
            Ok(stats) => {
                let mut ids: Vec<i64> = stats
                    .iter()
                    .filter_map(|(repo_id, _, _)| repo_id.map(|r| r.id() as i64))
                    .collect();
                ids.sort_unstable();
                ids.dedup();
                ids
            }
            Err(e) => {
                warn!("Failed to load repos for backfill {}: {:#}", entry.id.0, e);
                Vec::new()
            }
        };
        BackfillListRow {
            request_id: entry.id,
            created_at: entry.created_at,
            created_by: entry.created_by,
            aggregate_status,
            has_failed_requests,
            repo_count: entry.repo_count,
            repo_ids,
            derived_data_type,
        }
    }))
    .buffered(PARAMS_LOAD_CONCURRENCY)
    .collect()
    .await;

    // Resolve repo id -> name for every repo in the list. Names come from the
    // loaded repo configs; git repos aren't in the prod config tier, so for any
    // leftover ids fall back to the git source-of-truth table.
    let resolved = resolve_repo_names(ctx, app, repo_names, &rows).await;

    display_backfill_list(&rows, &resolved, all_active);

    Ok(())
}

/// Build an id -> name map covering every repo referenced by `rows`. Resolves
/// from the loaded repo configs first, then falls back to the git
/// source-of-truth table (which knows git repos absent from the config tier).
async fn resolve_repo_names(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_names: &HashMap<RepositoryId, String>,
    rows: &[BackfillListRow],
) -> HashMap<i64, String> {
    let mut resolved: HashMap<i64, String> = HashMap::new();
    let mut has_unresolved = false;
    for row in rows {
        for &repo_id in &row.repo_ids {
            if resolved.contains_key(&repo_id) {
                continue;
            }
            match i32::try_from(repo_id)
                .ok()
                .and_then(|id| repo_names.get(&RepositoryId::new(id)))
            {
                Some(name) => {
                    resolved.insert(repo_id, name.clone());
                }
                None => has_unresolved = true,
            }
        }
    }

    if has_unresolved {
        match load_git_repo_names(ctx, app, repo_names).await {
            Ok(git_names) => {
                for row in rows {
                    for &repo_id in &row.repo_ids {
                        if let std::collections::hash_map::Entry::Vacant(e) =
                            resolved.entry(repo_id)
                        {
                            if let Some(name) = git_names.get(&repo_id) {
                                e.insert(name.clone());
                            }
                        }
                    }
                }
            }
            Err(e) => warn!(
                "Failed to load git repo names from source of truth: {:#}",
                e
            ),
        }
    }

    resolved
}

/// Load id -> name for git repos from the source-of-truth table (git repos
/// aren't in the prod config tier). Covers every SoT state so we resolve as many
/// ids as possible. The git source-of-truth config is global, so we open it via
/// any configured repo.
async fn load_git_repo_names(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_names: &HashMap<RepositoryId, String>,
) -> Result<HashMap<i64, String>> {
    let any_repo_id = repo_names
        .keys()
        .next()
        .map(|repo_id| repo_id.id())
        .context("no configured repo available to open git source-of-truth config")?;

    let repo: GitSotRepo = app
        .open_repo(&RepoArgs::from_repo_id(any_repo_id))
        .await
        .context("opening repo for git source-of-truth lookup")?;

    let entries = repo
        .git_source_of_truth_config()
        .get_any(ctx)
        .await
        .context("listing git repos from source of truth")?;

    Ok(entries
        .into_iter()
        .filter(|entry| !entry.repo_name.0.is_empty())
        .map(|entry| (entry.repo_id.id() as i64, entry.repo_name.0))
        .collect())
}

async fn show_backfill_detail(
    ctx: &CoreContext,
    app: &MononokeApp,
    queue: &impl LongRunningRequestsQueue,
    blobstore: &Arc<dyn Blobstore>,
    repo_names: &HashMap<RepositoryId, String>,
    row_id: &RowId,
    repo: Option<&Repo>,
    manager: Option<&DerivedDataManager>,
    detailed: bool,
    max_log_lines: usize,
) -> Result<()> {
    // Step 1: Verify the backfill exists
    let root_entry = queue
        .get_backfill_root_entry(ctx, row_id)
        .await
        .context("fetching backfill root entry")?;

    let (request_id, request_type, root_status, created_at, args_blobstore_key, created_by) =
        match root_entry {
            Some(entry) => entry,
            None => {
                show_backfill_child_request_detail(
                    ctx,
                    queue,
                    blobstore,
                    repo_names,
                    row_id,
                    repo,
                    manager,
                    detailed,
                    max_log_lines,
                )
                .await?;
                return Ok(());
            }
        };

    let (derived_data_type, settings) =
        load_backfill_params_info(ctx, blobstore, &args_blobstore_key).await;

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

    let aggregate_status =
        RepoStatus::from_root_and_children(root_status, ChildCounts::from_status_map(&status_map));

    // Convert status_map to sorted vec
    let mut status_counts: Vec<(RequestStatus, usize)> = status_map.into_iter().collect();
    status_counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

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
        created_by,
        aggregate_status,
        request_type: request_type.to_string(),
        derived_data_type,
        settings,
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
        display_single_repo_detail(&data, repo_id, repo_names);

        if detailed {
            let (mut child_rows, new_count) =
                load_child_request_rows(ctx, queue, row_id, None).await?;
            display_child_request_table(&mut child_rows, new_count, false, repo_names);
        }
    } else if let Some(r) = repo {
        let drilldown_repo_id = r.repo_identity().id().id() as i64;
        show_repo_detail(
            ctx,
            queue,
            blobstore,
            repo_names,
            row_id,
            drilldown_repo_id,
            detailed,
        )
        .await?;
    } else {
        // Multi-repo backfill: show condensed view
        let total_repos = unique_repos.len();

        // Group repos by status
        let mut repo_status_map: HashMap<i64, HashMap<RequestStatus, usize>> = HashMap::new();
        for (repo_id_opt, req_status, count) in &stats_by_repo {
            if let Some(repo_id) = repo_id_opt {
                repo_status_map
                    .entry(repo_id.id() as i64)
                    .or_insert_with(HashMap::new)
                    .insert(req_status.clone(), *count as usize);
            }
        }

        let mut repos_by_status: HashMap<RepoStatus, Vec<i64>> = HashMap::new();
        for (repo_id, statuses) in &repo_status_map {
            let repo_status = RepoStatus::from_child_counts(ChildCounts::from_status_map(statuses));
            repos_by_status
                .entry(repo_status)
                .or_insert_with(Vec::new)
                .push(*repo_id);
        }

        let repos_by_status_counts = vec![
            (
                "Completed".to_string(),
                repos_by_status
                    .get(&RepoStatus::Completed)
                    .map(|v| v.len())
                    .unwrap_or(0),
            ),
            (
                "In Progress".to_string(),
                repos_by_status
                    .get(&RepoStatus::InProgress)
                    .map(|v| v.len())
                    .unwrap_or(0),
            ),
            (
                "Not Started".to_string(),
                repos_by_status
                    .get(&RepoStatus::NotStarted)
                    .map(|v| v.len())
                    .unwrap_or(0),
            ),
            (
                "Failed".to_string(),
                repos_by_status
                    .get(&RepoStatus::Failed)
                    .map(|v| v.len())
                    .unwrap_or(0),
            ),
        ];

        // Get failed repos with counts
        let failed_repos: Vec<(i64, usize)> = repos_by_status
            .get(&RepoStatus::Failed)
            .map(|repos| {
                repos
                    .iter()
                    .map(|repo_id| {
                        let failed_count = repo_status_map
                            .get(repo_id)
                            .and_then(|s| s.get(&RequestStatus::Failed))
                            .copied()
                            .unwrap_or(0);
                        (*repo_id, failed_count)
                    })
                    .collect()
            })
            .unwrap_or_default();

        display_multi_repo_summary(
            &data,
            total_repos,
            &repos_by_status_counts,
            &failed_repos,
            repo_names,
        );

        if detailed {
            let mut detail_rows = load_per_repo_commit_counts(
                ctx,
                app,
                queue,
                blobstore,
                repo_names,
                row_id,
                &repo_status_map,
            )
            .await?;
            display_repo_detail_table(&mut detail_rows);

            // Also show the per-child-request breakdown (as for single-repo
            // backfills), with a Repo column so each request can be attributed
            // to its repository.
            let (mut child_rows, new_count) =
                load_child_request_rows(ctx, queue, row_id, None).await?;
            display_child_request_table(&mut child_rows, new_count, true, repo_names);
        }
    }

    Ok(())
}

/// Load per-repo commit counts: "derived" from completed child request
/// results, "total" from the phases table (public commit count per repo).
async fn load_per_repo_commit_counts(
    ctx: &CoreContext,
    app: &MononokeApp,
    queue: &impl LongRunningRequestsQueue,
    blobstore: &Arc<dyn Blobstore>,
    repo_names: &HashMap<RepositoryId, String>,
    row_id: &RowId,
    repo_status_map: &HashMap<i64, HashMap<RequestStatus, usize>>,
) -> Result<Vec<RepoDetailRow>> {
    let entries = queue
        .get_requests_by_root_id(ctx, row_id)
        .await
        .context("fetching child entries for detailed view")?;

    // Load derived counts from completed derive_boundaries/derive_slice results
    let derived_pairs: Vec<(i64, i64)> = stream::iter(entries.iter().filter(|e| {
        (e.request_type.0 == DeriveBoundaries::NAME || e.request_type.0 == DeriveSlice::NAME)
            && matches!(e.status, RequestStatus::Ready | RequestStatus::Polled)
            && e.result_blobstore_key.is_some()
    }))
    .map(|entry| async {
        let result = match load_child_result(ctx, blobstore, entry).await {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to load result for request {}: {:#}", entry.id.0, e);
                return None;
            }
        };
        let repo_id = entry.repo_id.map(|r| r.id() as i64)?;
        let derived_count = match result? {
            BackfillChildResult::DeriveBoundaries { derived_count, .. }
            | BackfillChildResult::DeriveSlice { derived_count, .. } => derived_count.max(0),
            BackfillChildResult::Error { .. } => 0,
        };
        Some((repo_id, derived_count))
    })
    .buffer_unordered(PARAMS_LOAD_CONCURRENCY)
    .filter_map(|x| async move { x })
    .collect()
    .await;

    let mut repo_derived: HashMap<i64, i64> = HashMap::new();
    for (repo_id, count) in derived_pairs {
        *repo_derived.entry(repo_id).or_default() += count;
    }

    // Load total public commit counts and repo names from the phases table.
    // We get repo names here (from RepoIdentity) rather than relying on the
    // caller's repo_names map, which may not include all repos.
    let repo_ids: Vec<i64> = repo_status_map.keys().copied().collect();
    let repo_info: HashMap<i64, (String, u64)> = stream::iter(repo_ids.iter())
        .map(|repo_id| async move {
            let repo_id_i32 = match i32::try_from(*repo_id) {
                Ok(id) => id,
                Err(_) => {
                    warn!("Repo id {} out of range, skipping phase count", repo_id);
                    return None;
                }
            };
            let repo_args = RepoArgs::from_repo_id(repo_id_i32);
            let phase_repo: PhaseCountRepo = match app.open_repo(&repo_args).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("Failed to open repo {} for phase count: {:#}", repo_id, e);
                    return None;
                }
            };
            let name = phase_repo.repo_identity().name().to_string();
            let count = match phase_repo
                .phases()
                .count_all_public(ctx, RepositoryId::new(repo_id_i32))
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    warn!(
                        "Failed to count public commits for repo {}: {:#}",
                        repo_id, e
                    );
                    return None;
                }
            };
            Some((*repo_id, (name, count)))
        })
        .buffer_unordered(PARAMS_LOAD_CONCURRENCY)
        .filter_map(|x| async move { x })
        .collect()
        .await;

    Ok(repo_status_map
        .iter()
        .map(|(repo_id, statuses)| {
            let status = RepoStatus::from_child_counts(ChildCounts::from_status_map(statuses));
            let (repo_name, total) = match repo_info.get(repo_id) {
                Some((name, count)) => (Some(name.clone()), *count as usize),
                None => (
                    i32::try_from(*repo_id)
                        .ok()
                        .and_then(|id| repo_names.get(&RepositoryId::new(id)))
                        .cloned(),
                    0,
                ),
            };
            let derived = *repo_derived.get(repo_id).unwrap_or(&0) as usize;
            RepoDetailRow {
                repo_id: *repo_id,
                repo_name,
                status,
                derived,
                total,
            }
        })
        .collect())
}

/// Load all child requests of a backfill for the single-repo detailed view.
///
/// Returns the rows to render (every request that has been claimed or has
/// progressed past `new`) along with the count of `new` requests, which are
/// elided from the table: a large repo backfill can have thousands of them
/// sitting in the queue, so we just report the count rather than listing each.
async fn load_child_request_rows(
    ctx: &CoreContext,
    queue: &impl LongRunningRequestsQueue,
    row_id: &RowId,
    repo_filter: Option<i64>,
) -> Result<(Vec<ChildRequestRow>, usize)> {
    let entries = queue
        .get_requests_by_root_id(ctx, row_id)
        .await
        .context("fetching child entries for detailed view")?;

    // When drilling into a single repo of a multi-repo backfill, keep only that
    // repo's child requests; otherwise consider all of them.
    let matches_repo = |entry_repo: Option<RepositoryId>| match repo_filter {
        Some(repo_id) => entry_repo.map(|r| r.id() as i64) == Some(repo_id),
        None => true,
    };

    let new_count = entries
        .iter()
        .filter(|entry| entry.status == RequestStatus::New && matches_repo(entry.repo_id))
        .count();

    let rows = entries
        .iter()
        .filter(|entry| entry.status != RequestStatus::New && matches_repo(entry.repo_id))
        .map(|entry| ChildRequestRow {
            id: entry.id.0,
            repo_id: entry.repo_id.map(|r| r.id() as i64),
            request_type: entry.request_type.0.clone(),
            status: entry.status,
            claimed_by: entry.claimed_by.as_ref().map(|c| c.0.clone()),
        })
        .collect();

    Ok((rows, new_count))
}

/// SMC tier serving Tupperware task logs (same backend as the `tw log` CLI).
#[cfg(fbcode_build)]
const TW_LOG_READER_TIER: &str = "tupperware.log_reader";
/// Max bytes to pull per `readLogLines` round-trip.
#[cfg(fbcode_build)]
const WORKER_LOG_READ_RESPONSE_SIZE: i32 = 4 * 1024 * 1024;
/// Lines matching this are noise we don't want when debugging a backfill
/// child request; filtered out server-side via an inverted `LineContentFilter`.
#[cfg(fbcode_build)]
const WORKER_LOG_EXCLUDE_PATTERN: &str = r"\[warm_bookmarks_cache\]";
/// Pad the processing window on both ends to catch setup/teardown lines.
#[cfg(fbcode_build)]
const WORKER_LOG_WINDOW_BUFFER_SECS: i64 = 30;

/// The TW `user` component the async-requests backfill worker runs as. Used to
/// repair legacy handles that are missing it (see `parse_tw_task_handle`).
#[cfg(fbcode_build)]
const BACKFILL_WORKER_TW_USER: &str = "mononoke";

/// Split a `claimed_by` value into a Tupperware job handle (`cluster/user/name`)
/// and task id (e.g. `tsp_prn/mononoke/backfill_worker/31`).
///
/// Workaround: handles written before D108147656 are missing the `user`
/// component, so they look like `tsp_prn/backfill_worker/31`
/// (`cluster/name/task`) instead of `tsp_prn/mononoke/backfill_worker/31`
/// (`cluster/user/name/task`). The LogReader rejects the former with
/// "Handle must take the form cluster/user/name", so when we see a 2-component
/// (`cluster/name`) job handle we re-insert the default `mononoke` user. This
/// can be dropped once all stale db rows have aged out.
#[cfg(fbcode_build)]
fn parse_tw_task_handle(handle: &str) -> Option<(String, i32)> {
    let (job_handle, task) = handle.rsplit_once('/')?;
    let task_id = task.parse::<i32>().ok()?;

    let job_handle = match job_handle.split('/').collect::<Vec<_>>().as_slice() {
        [cluster, name] => format!("{cluster}/{BACKFILL_WORKER_TW_USER}/{name}"),
        _ => job_handle.to_string(),
    };
    Some((job_handle, task_id))
}

/// Fetch up to `max_lines` of the most recent `stderr` for a single child
/// request over `[start_ts, end_ts]` (epoch seconds), dropping
/// `warm_bookmarks_cache` noise server-side.
///
/// Returns the chronological tail plus a `truncated` flag that is true when more
/// (older) lines exist in the window than were returned. Since LogReader streams
/// lines newest-first, we stop reading as soon as we have enough for the tail
/// rather than loading the whole window.
#[cfg(fbcode_build)]
async fn fetch_worker_logs(
    fb: FacebookInit,
    job_handle: &str,
    task_id: i32,
    start_ts: i64,
    end_ts: i64,
    max_lines: usize,
) -> Result<(Vec<String>, bool)> {
    let client = make_LogReader_srclient!(fb, tiername = TW_LOG_READER_TIER)?;

    let source_id = SourceIdentity::taskId(TaskIdentity {
        jobHandle: JobHandle {
            handle: job_handle.to_string(),
            ..Default::default()
        },
        taskID: task_id,
        ..Default::default()
    });

    let session = client
        .getSession(&GetSessionRequest {
            sourceId: source_id,
            filePath: "stderr".to_string(),
            timestamp: Some(end_ts),
            filter: Some(LogFilter {
                startTimestamp: Some(start_ts),
                endTimestamp: Some(end_ts),
                // `invertMatch` keeps only lines that do NOT match the pattern,
                // so this filters warm_bookmarks_cache lines out.
                lineContentFilters: Some(vec![LineContentFilter {
                    pattern: WORKER_LOG_EXCLUDE_PATTERN.to_string(),
                    invertMatch: true,
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .context("opening log reader session")?
        .session;

    // Read BACKWARD from `end_ts` toward `start_ts` (how historical windows are
    // read — FORWARD is for live tailing and from `end_ts` would read past the
    // window and return nothing). Each response is a chunk going further back in
    // time, so we stop once we've gathered enough for the tail, then reverse the
    // chunk order to restore chronological output.
    let mut next_session = Some(session);
    let mut batches: Vec<Vec<String>> = Vec::new();
    let mut collected = 0usize;
    let mut truncated = false;
    while let Some(session) = next_session {
        let resp = client
            .readLogLines(&ReadLogLinesRequest {
                session,
                direction: LogDirection::BACKWARD,
                responseSizeBytes: WORKER_LOG_READ_RESPONSE_SIZE,
                ..Default::default()
            })
            .await
            .context("reading log lines")?;
        let batch: Vec<String> = resp
            .lines
            .into_iter()
            .map(|l| String::from_utf8_lossy(&l.line).trim_end().to_string())
            .collect();
        collected += batch.len();
        batches.push(batch);
        next_session = resp.session;

        if collected >= max_lines {
            // Enough for the tail. Anything still unread, or the surplus we'll
            // trim below, is older than the tail and counts as omitted.
            truncated = next_session.is_some() || collected > max_lines;
            break;
        }
    }

    batches.reverse();
    let mut lines: Vec<String> = batches.into_iter().flatten().collect();
    if lines.len() > max_lines {
        lines.drain(0..lines.len() - max_lines);
    }
    Ok((lines, truncated))
}

/// Print the claiming worker's Tupperware logs for a single child request,
/// scoped to that request's processing window. Best-effort: a failure to fetch
/// the logs is reported inline rather than aborting the status command.
#[cfg(fbcode_build)]
async fn show_worker_logs_for_entry(
    ctx: &CoreContext,
    entry: &LongRunningRequestEntry,
    max_log_lines: usize,
) -> Result<()> {
    println!();
    println!("Worker Logs:");
    println!("{}", "━".repeat(80));

    let Some(claimed_by) = entry.claimed_by.as_ref().map(|c| c.0.as_str()) else {
        println!("  Request has not been claimed by a worker yet — no logs to show.");
        println!("{}", "━".repeat(80));
        return Ok(());
    };

    let Some((job_handle, task_id)) = parse_tw_task_handle(claimed_by) else {
        println!("  Could not parse worker task handle from '{claimed_by}'.");
        println!("{}", "━".repeat(80));
        return Ok(());
    };

    let Some(start_secs) = entry
        .started_processing_at
        .as_ref()
        .map(|t| t.timestamp_seconds())
    else {
        println!("  No processing window recorded for this request yet.");
        println!("{}", "━".repeat(80));
        return Ok(());
    };
    // End at the request's terminal timestamp if it has one, else "now" for a
    // still-running request.
    let end_secs = entry
        .failed_at
        .as_ref()
        .or(entry.ready_at.as_ref())
        .map(|t| t.timestamp_seconds())
        .unwrap_or_else(|| Timestamp::now().timestamp_seconds());

    let start_ts = start_secs - WORKER_LOG_WINDOW_BUFFER_SECS;
    let end_ts = end_secs + WORKER_LOG_WINDOW_BUFFER_SECS;

    println!("  Worker:  {claimed_by}");
    println!(
        "  Window:  {} → {}",
        format_timestamp(&Timestamp::from_timestamp_secs(start_secs)),
        format_timestamp(&Timestamp::from_timestamp_secs(end_secs)),
    );
    println!("  File:    stderr (excluding warm_bookmarks_cache)");
    println!();

    match fetch_worker_logs(
        ctx.fb,
        &job_handle,
        task_id,
        start_ts,
        end_ts,
        max_log_lines,
    )
    .await
    {
        Ok((lines, _)) if lines.is_empty() => {
            println!("  (no log lines in window — may be GC'd, or all filtered out)");
        }
        Ok((lines, truncated)) => {
            if truncated {
                println!(
                    "  (showing the last {} lines; earlier lines in this window omitted)",
                    lines.len()
                );
                println!();
            }
            for line in &lines {
                println!("  {line}");
            }
            if truncated {
                println!("{}", "━".repeat(80));
                println!("To see the full window, run:");
                println!(
                    "    tw log {job_handle}/{task_id} -s \"{}\" -e \"{}\" -p \"{}\" -v",
                    format_timestamp(&Timestamp::from_timestamp_secs(start_ts)),
                    format_timestamp(&Timestamp::from_timestamp_secs(end_ts)),
                    WORKER_LOG_EXCLUDE_PATTERN,
                );
            }
        }
        Err(e) => println!("  (failed to fetch worker logs: {e:#})"),
    }
    println!("{}", "━".repeat(80));
    Ok(())
}

/// OSS builds can't reach Tupperware's log reader service.
#[cfg(not(fbcode_build))]
async fn show_worker_logs_for_entry(
    _ctx: &CoreContext,
    _entry: &LongRunningRequestEntry,
    _max_log_lines: usize,
) -> Result<()> {
    println!();
    println!("Worker logs are only available in fbcode builds.");
    Ok(())
}

fn format_changeset_id(bytes: &[u8]) -> String {
    ChangesetId::from_bytes(bytes)
        .map(|cs_id| cs_id.to_string())
        .unwrap_or_else(|e| format!("<invalid changeset id: {e}>"))
}

fn parse_changeset_id(bytes: &[u8]) -> Result<ChangesetId> {
    ChangesetId::from_bytes(bytes).context("parsing changeset id")
}

fn decode_child_params(
    entry: &LongRunningRequestEntry,
    params: &AsynchronousRequestParams,
) -> Result<BackfillChildParams> {
    match (entry.request_type.0.as_str(), params.thrift()) {
        (DeriveBoundaries::NAME, ThriftAsynchronousRequestParams::derive_boundaries_params(p)) => {
            Ok(BackfillChildParams::DeriveBoundaries {
                repo_id: p.repo_id,
                derived_data_type: p.derived_data_type.clone(),
                boundary_cs_ids: p
                    .boundary_cs_ids
                    .iter()
                    .map(|cs_id| parse_changeset_id(cs_id.as_ref()))
                    .collect::<Result<Vec<_>>>()
                    .context("parsing derive_boundaries boundary changeset ids")?,
                concurrency: p.concurrency,
                use_predecessor_derivation: p.use_predecessor_derivation,
                config_name: p.config_name.clone(),
            })
        }
        (DeriveSlice::NAME, ThriftAsynchronousRequestParams::derive_slice_params(p)) => {
            Ok(BackfillChildParams::DeriveSlice {
                repo_id: p.repo_id,
                derived_data_type: p.derived_data_type.clone(),
                segments: p
                    .segments
                    .iter()
                    .map(|segment| SliceSegmentDisplayData {
                        head: format_changeset_id(segment.head.as_ref()),
                        base: format_changeset_id(segment.base.as_ref()),
                    })
                    .collect(),
                config_name: p.config_name.clone(),
            })
        }
        (request_type, _) => bail!(
            "Request ID {} has type {}, not derive_boundaries or derive_slice",
            entry.id.0,
            request_type,
        ),
    }
}

async fn load_child_result(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    entry: &LongRunningRequestEntry,
) -> Result<Option<BackfillChildResult>> {
    let Some(result_key) = &entry.result_blobstore_key else {
        return Ok(None);
    };

    let result = AsynchronousRequestResult::load_from_key(ctx, blobstore, &result_key.0)
        .await
        .context("loading request result")?;

    match (entry.request_type.0.as_str(), result.thrift()) {
        (
            DeriveBoundaries::NAME,
            ThriftAsynchronousRequestResult::derive_boundaries_result(result),
        ) => Ok(Some(BackfillChildResult::DeriveBoundaries {
            derived_count: result.derived_count,
            error_message: result.error_message.clone(),
        })),
        (DeriveSlice::NAME, ThriftAsynchronousRequestResult::derive_slice_result(result)) => {
            Ok(Some(BackfillChildResult::DeriveSlice {
                derived_count: result.derived_count,
                error_message: result.error_message.clone(),
            }))
        }
        (_, ThriftAsynchronousRequestResult::error(error)) => {
            Ok(Some(BackfillChildResult::Error {
                message: format!("{error:?}"),
            }))
        }
        (request_type, result) => bail!(
            "Request ID {} has type {}, but result blob contains {:?}",
            entry.id.0,
            request_type,
            result,
        ),
    }
}

fn manager_with_all_types(manager: &DerivedDataManager) -> DerivedDataManager {
    let mut config = manager.config().clone();
    config.types = DerivableType::iter().collect();
    manager.with_replaced_config(manager.config_name(), config)
}

async fn load_boundary_derivation_status(
    ctx: &CoreContext,
    repo: Option<&Repo>,
    default_manager: Option<&DerivedDataManager>,
    params: &BackfillChildParams,
) -> Result<Option<BoundaryDerivationStatus>> {
    let BackfillChildParams::DeriveBoundaries {
        repo_id,
        derived_data_type,
        boundary_cs_ids,
        config_name,
        ..
    } = params
    else {
        return Ok(None);
    };

    let Some(repo) = repo else {
        return Ok(None);
    };

    let request_repo_id = match i32::try_from(*repo_id) {
        Ok(repo_id) => RepositoryId::new(repo_id),
        Err(_) => {
            return Ok(Some(BoundaryDerivationStatus::NotChecked {
                reason: format!("request repo id {repo_id} is out of range"),
            }));
        }
    };
    let opened_repo_id = repo.repo_identity().id();
    if request_repo_id != opened_repo_id {
        return Ok(Some(BoundaryDerivationStatus::NotChecked {
            reason: format!(
                "request repo {} does not match opened repo {}",
                repo_id,
                opened_repo_id.id()
            ),
        }));
    }

    let manager = match config_name {
        Some(config_name) => {
            let manager = repo
                .repo_derived_data()
                .manager_for_config(config_name)
                .with_context(|| format!("loading derived data config {config_name}"))?;
            manager_with_all_types(manager)
        }
        None => default_manager
            .cloned()
            .context("derived data manager unavailable for boundary derived status")?,
    };
    let derived_data_type = DerivableType::from_name(derived_data_type)
        .with_context(|| format!("resolving derived data type {derived_data_type}"))?;
    let not_derived =
        BulkDerivation::pending(&manager, ctx, boundary_cs_ids, None, derived_data_type)
            .await
            .context("checking pending boundary changesets")?;
    let not_derived_count = not_derived.len();

    Ok(Some(BoundaryDerivationStatus::Checked {
        already_derived_count: boundary_cs_ids.len().saturating_sub(not_derived_count),
        not_derived_count,
    }))
}

async fn show_backfill_child_request_detail(
    ctx: &CoreContext,
    queue: &impl LongRunningRequestsQueue,
    blobstore: &Arc<dyn Blobstore>,
    repo_names: &HashMap<RepositoryId, String>,
    row_id: &RowId,
    repo: Option<&Repo>,
    manager: Option<&DerivedDataManager>,
    detailed: bool,
    max_log_lines: usize,
) -> Result<()> {
    let entry = queue
        .get_request_entry_by_id(ctx, row_id)
        .await
        .context("fetching request entry")?
        .ok_or_else(|| anyhow::anyhow!("Invalid request ID: {} does not exist", row_id.0))?;

    if entry.request_type.0 != DeriveBoundaries::NAME && entry.request_type.0 != DeriveSlice::NAME {
        bail!(
            "Invalid request ID: {} is not a backfill root, derive_boundaries, or derive_slice request",
            row_id.0
        );
    }

    let params =
        AsynchronousRequestParams::load_from_key(ctx, blobstore, &entry.args_blobstore_key.0)
            .await
            .context("loading request params")?;
    let params = decode_child_params(&entry, &params)?;
    let result = load_child_result(ctx, blobstore, &entry).await?;
    let boundary_derivation_status =
        load_boundary_derivation_status(ctx, repo, manager, &params).await?;

    let data = BackfillChildDisplayData {
        entry,
        params,
        result,
        boundary_derivation_status,
    };
    display_child_request_detail(&data, repo_names);

    // With --detailed, also pull the claiming worker's logs for this request.
    if detailed {
        show_worker_logs_for_entry(ctx, &data.entry, max_log_lines).await?;
    }

    Ok(())
}

async fn show_repo_detail(
    ctx: &CoreContext,
    queue: &impl LongRunningRequestsQueue,
    blobstore: &Arc<dyn Blobstore>,
    repo_names: &HashMap<RepositoryId, String>,
    row_id: &RowId,
    repo_id: i64,
    detailed: bool,
) -> Result<()> {
    // Verify the backfill exists
    let root_entry = queue
        .get_backfill_root_entry(ctx, row_id)
        .await
        .context("fetching backfill root entry")?;

    let (request_id, _request_type, _status, _created_at, args_blobstore_key, _created_by) =
        match root_entry {
            Some(entry) => entry,
            None => bail!(
                "Invalid request ID: {} is not a backfill root request",
                row_id.0
            ),
        };

    let derived_data_type = load_derived_data_type(ctx, blobstore, &args_blobstore_key).await;

    // Get stats for this specific repo
    let repo_id_i32 =
        i32::try_from(repo_id).context("repo_id out of range for RepositoryId (i32)")?;
    let repo_id_typed = RepositoryId::new(repo_id_i32);
    let repo_stats = queue
        .get_backfill_stats(ctx, row_id, Some(&repo_id_typed))
        .await
        .context("fetching repo stats")?;

    if repo_stats.is_empty() {
        bail!(
            "No data found for repo {} in backfill {}",
            repo_id,
            request_id.0
        );
    }

    // Count by status
    let mut status_map: HashMap<RequestStatus, usize> = HashMap::new();
    let mut total_requests = 0;
    for (_req_type, req_status, count) in &repo_stats {
        *status_map.entry(*req_status).or_insert(0) += *count as usize;
        total_requests += *count as usize;
    }

    let mut status_counts: Vec<(RequestStatus, usize)> =
        status_map.iter().map(|(s, c)| (*s, *c)).collect();
    status_counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

    // Group by request type
    let mut type_map: HashMap<String, Vec<(RequestStatus, usize)>> = HashMap::new();
    for (req_type, req_status, count) in &repo_stats {
        type_map
            .entry(req_type.0.clone())
            .or_insert_with(Vec::new)
            .push((*req_status, *count as usize));
    }
    let mut type_breakdown: Vec<(String, Vec<(RequestStatus, usize)>)> =
        type_map.into_iter().collect();
    type_breakdown.sort_by(|a, b| a.0.cmp(&b.0));

    let overall_status = RepoStatus::from_child_counts(ChildCounts::from_status_map(&status_map));
    let repo_name = i32::try_from(repo_id)
        .ok()
        .and_then(|id| repo_names.get(&RepositoryId::new(id)))
        .cloned();

    display_repo_detail(&RepoDisplayData {
        request_id,
        repo_id,
        repo_name,
        overall_status,
        derived_data_type,
        total_requests,
        status_counts,
        type_breakdown,
    });

    if detailed {
        // Show the per-child-request breakdown for this repo, matching the
        // single-repo detailed view. Every request belongs to this repo, so the
        // Repo column is omitted.
        let (mut child_rows, new_count) =
            load_child_request_rows(ctx, queue, row_id, Some(repo_id)).await?;
        display_child_request_table(&mut child_rows, new_count, false, repo_names);
    }

    Ok(())
}
