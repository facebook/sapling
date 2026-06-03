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
use futures::stream;
use futures::stream::StreamExt;
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
use requests_table::RequestStatus;
use requests_table::RowId;
use requests_table::SqlLongRunningRequestsQueue;
use strum::IntoEnumIterator;
use tracing::warn;

use self::display::BackfillListRow;
use self::display::display_backfill_list;
use self::display::display_child_request_detail;
use self::display::display_multi_repo_summary;
use self::display::display_repo_detail;
use self::display::display_repo_detail_table;
use self::display::display_single_repo_detail;
use self::types::BackfillChildDisplayData;
use self::types::BackfillChildParams;
use self::types::BackfillChildResult;
use self::types::BackfillDisplayData;
use self::types::BoundaryDerivationStatus;
use self::types::ChildCounts;
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

    /// Show per-repository progress details for multi-repo backfills
    #[clap(long)]
    detailed: bool,
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
            list_backfills(ctx, &queue, &blobstore, args.lookback).await?;
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

async fn list_backfills(
    ctx: &CoreContext,
    queue: &impl LongRunningRequestsQueue,
    blobstore: &Arc<dyn Blobstore>,
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
        let has_active_work = matches!(
            entry.root_status,
            RequestStatus::New | RequestStatus::InProgress
        ) || children.new > 0
            || children.inprogress > 0;
        let aggregate_status = if has_failed_requests && has_active_work {
            RepoStatus::InProgress
        } else {
            RepoStatus::from_root_and_children(entry.root_status, children)
        };
        BackfillListRow {
            request_id: entry.id,
            created_at: entry.created_at,
            created_by: entry.created_by,
            aggregate_status,
            has_failed_requests,
            repo_count: entry.repo_count,
            derived_data_type,
        }
    }))
    .buffered(PARAMS_LOAD_CONCURRENCY)
    .collect()
    .await;

    display_backfill_list(&rows);

    Ok(())
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
                    ctx, queue, blobstore, repo_names, row_id, repo, manager,
                )
                .await?;
                return Ok(());
            }
        };

    let derived_data_type = load_derived_data_type(ctx, blobstore, &args_blobstore_key).await;

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
    } else if let Some(r) = repo {
        let drilldown_repo_id = r.repo_identity().id().id() as i64;
        show_repo_detail(ctx, queue, blobstore, repo_names, row_id, drilldown_repo_id).await?;
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

fn format_changeset_id(bytes: &[u8]) -> String {
    ChangesetId::from_bytes(bytes)
        .map(|cs_id| cs_id.to_string())
        .unwrap_or_else(|e| format!("<invalid changeset id: {}>", e))
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
                message: format!("{:?}", error),
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
                reason: format!("request repo id {} is out of range", repo_id),
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
                .with_context(|| format!("loading derived data config {}", config_name))?;
            manager_with_all_types(manager)
        }
        None => default_manager
            .cloned()
            .context("derived data manager unavailable for boundary derived status")?,
    };
    let derived_data_type = DerivableType::from_name(derived_data_type)
        .with_context(|| format!("resolving derived data type {}", derived_data_type))?;
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

    display_child_request_detail(
        &BackfillChildDisplayData {
            entry,
            params,
            result,
            boundary_derivation_status,
        },
        repo_names,
    );

    Ok(())
}

async fn show_repo_detail(
    ctx: &CoreContext,
    queue: &impl LongRunningRequestsQueue,
    blobstore: &Arc<dyn Blobstore>,
    repo_names: &HashMap<RepositoryId, String>,
    row_id: &RowId,
    repo_id: i64,
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

    Ok(())
}
