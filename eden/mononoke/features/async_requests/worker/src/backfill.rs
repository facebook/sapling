/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU32;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Error;
use anyhow::Result;
use async_requests::AsyncMethodRequestQueue;
use async_requests::AsyncRequestsError;
use async_requests::types::RowId;
use async_requests::types::Token;
use bulk_derivation::BulkDerivation;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use derived_data_manager::DerivedDataManager;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use futures_stats::TimedTryFutureExt;
use mononoke_api::Mononoke;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::RepositoryId;
use repo_authorization::AuthorizationContext;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedDataRef;
use source_control as thrift;
use throttledblob::ThrottleOptions;
use throttledblob::ThrottledBlob;
use tracing::info;

/// JustKnob for write QPS throttling during backfill derivation
const JK_BACKFILL_WRITE_QPS: &str = "scm/mononoke:derived_data_backfill_write_qps";
/// JustKnob for write bytes/s throttling during backfill derivation
const JK_BACKFILL_WRITE_BYTES_S: &str = "scm/mononoke:derived_data_backfill_write_bytes_s";
/// JustKnob for read QPS throttling during backfill derivation
const JK_BACKFILL_READ_QPS: &str = "scm/mononoke:derived_data_backfill_read_qps";
/// JustKnob for read bytes/s throttling during backfill derivation
const JK_BACKFILL_READ_BYTES_S: &str = "scm/mononoke:derived_data_backfill_read_bytes_s";

/// JustKnob for number of segments to derive in parallel
const JK_BACKFILL_SEGMENT_CONCURRENCY: &str =
    "scm/mononoke:derived_data_backfill_segment_concurrency";
/// JustKnob for chunk size when deriving slices of commits
const JK_BACKFILL_SEGMENT_CHUNK_SIZE: &str =
    "scm/mononoke:derived_data_backfill_segment_chunk_size";

/// Get a DerivedDataManager with optional read/write throttling applied.
/// If JustKnobs are not set or are zero, returns the manager unchanged.
fn get_throttled_manager(manager: &DerivedDataManager) -> Result<DerivedDataManager> {
    let write_qps = justknobs::get_as::<i64>(JK_BACKFILL_WRITE_QPS, None)?;
    let write_bytes = justknobs::get_as::<i64>(JK_BACKFILL_WRITE_BYTES_S, None)?;
    let read_qps = justknobs::get_as::<i64>(JK_BACKFILL_READ_QPS, None)?;
    let read_bytes = justknobs::get_as::<i64>(JK_BACKFILL_READ_BYTES_S, None)?;

    if write_qps <= 0 && write_bytes <= 0 && read_qps <= 0 && read_bytes <= 0 {
        return Ok(manager.clone());
    }

    let options = ThrottleOptions {
        write_qps: NonZeroU32::new(write_qps.max(0) as u32),
        write_bytes: NonZeroUsize::new(write_bytes.max(0) as usize),
        read_qps: NonZeroU32::new(read_qps.max(0) as u32),
        read_bytes: NonZeroUsize::new(read_bytes.max(0) as usize),
        ..Default::default()
    };

    info!(
        "Applying throttle: write_qps={:?}, write_bytes/s={:?}, read_qps={:?}, read_bytes/s={:?}",
        options.write_qps, options.write_bytes, options.read_qps, options.read_bytes,
    );

    let repo_blobstore = manager.repo_blobstore().clone();
    let throttled_blobstore =
        RepoBlobstore::new_with_wrapped_inner_blobstore(repo_blobstore, |inner| {
            Arc::new(ThrottledBlob::new(inner, options))
        });
    Ok(manager.with_replaced_blobstore(throttled_blobstore))
}

/// Returns a manager with the given derived data type enabled in its config.
/// If the type is already enabled, returns a clone of the existing manager.
/// This is needed for backfill operations where the type may not yet be in the repo config.
fn with_type_enabled(
    manager: &DerivedDataManager,
    derived_data_type: DerivableType,
) -> DerivedDataManager {
    if manager.config().types.contains(&derived_data_type) {
        return manager.clone();
    }
    let mut config = manager.config().clone();
    config.types.insert(derived_data_type);
    manager.with_replaced_config(manager.config_name(), config)
}

/// Returns true if this derived data type requires slices to be chained
/// serially (each slice depends on the previous). Types that support
/// derive_from_predecessor can derive boundaries independently, allowing
/// parallel slice processing.
fn requires_serial_slice_processing(derived_data_type: DerivableType) -> bool {
    derived_data_type
        .into_derivable_untopologically_variant()
        .is_err()
}

/// Compute derive_boundaries request - derives boundary changesets using predecessor derivation
pub(crate) async fn compute_derive_boundaries(
    ctx: &CoreContext,
    mononoke: Arc<Mononoke<Repo>>,
    params: thrift::DeriveBoundariesParams,
) -> Result<thrift::DeriveBoundariesResponse, AsyncRequestsError> {
    let repo_id = RepositoryId::new(
        params
            .repo_id
            .try_into()
            .map_err(|e| AsyncRequestsError::request(anyhow::anyhow!("Invalid repo_id: {}", e)))?,
    );

    let repo = mononoke
        .repo_by_id(ctx.clone(), repo_id)
        .await
        .map_err(AsyncRequestsError::internal)?
        .ok_or_else(|| {
            AsyncRequestsError::request(anyhow::anyhow!("Repo not found: {}", params.repo_id))
        })?
        .with_authorization_context(AuthorizationContext::new_bypass_access_control())
        .build()
        .await
        .map_err(AsyncRequestsError::internal)?;

    let derived_data_type =
        DerivableType::from_name(&params.derived_data_type).map_err(AsyncRequestsError::request)?;

    let boundary_cs_ids: Vec<ChangesetId> = params
        .boundary_cs_ids
        .iter()
        .map(ChangesetId::from_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(AsyncRequestsError::request)?;

    info!(
        "Deriving {} boundary changesets for repo {} type {:?}",
        boundary_cs_ids.len(),
        params.repo_id,
        derived_data_type,
    );

    let derived_count = Arc::new(AtomicUsize::new(0));
    let manager = get_throttled_manager(repo.repo().repo_derived_data().manager())
        .map_err(AsyncRequestsError::internal)?;
    let manager = with_type_enabled(&manager, derived_data_type);
    let concurrency = params.concurrency.max(1) as usize;
    let use_predecessor = params.use_predecessor_derivation;

    stream::iter(boundary_cs_ids)
        .map(Ok::<_, Error>)
        .try_for_each_concurrent(concurrency, |csid| {
            let manager = manager.clone();
            let ctx = ctx.clone();
            let derived_count = derived_count.clone();
            async move {
                if use_predecessor {
                    BulkDerivation::unsafe_derive_untopologically(
                        &manager,
                        &ctx,
                        csid,
                        None, // rederivation
                        derived_data_type,
                    )
                    .await?;
                } else {
                    manager
                        .derive_bulk_locally(
                            &ctx,
                            &[csid],
                            None, // rederivation
                            &[derived_data_type],
                            None, // override_batch_size
                        )
                        .await
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                }
                derived_count.fetch_add(1, Ordering::SeqCst);
                Ok::<_, Error>(())
            }
        })
        .await
        .map_err(AsyncRequestsError::internal)?;

    let count = derived_count.load(Ordering::SeqCst) as i64;
    info!("Derived {} boundary changesets", count);

    Ok(thrift::DeriveBoundariesResponse {
        derived_count: count,
        error_message: None,
        ..Default::default()
    })
}

/// Compute derive_slice request - derives a slice of commits (segments defined by head..base ranges)
pub(crate) async fn compute_derive_slice(
    ctx: &CoreContext,
    mononoke: Arc<Mononoke<Repo>>,
    params: thrift::DeriveSliceParams,
) -> Result<thrift::DeriveSliceResponse, AsyncRequestsError> {
    let repo_id = RepositoryId::new(
        params
            .repo_id
            .try_into()
            .map_err(|e| AsyncRequestsError::request(anyhow::anyhow!("Invalid repo_id: {}", e)))?,
    );

    let repo = mononoke
        .repo_by_id(ctx.clone(), repo_id)
        .await
        .map_err(AsyncRequestsError::internal)?
        .ok_or_else(|| {
            AsyncRequestsError::request(anyhow::anyhow!("Repo not found: {}", params.repo_id))
        })?
        .with_authorization_context(AuthorizationContext::new_bypass_access_control())
        .build()
        .await
        .map_err(AsyncRequestsError::internal)?;

    let derived_data_type =
        DerivableType::from_name(&params.derived_data_type).map_err(AsyncRequestsError::request)?;

    info!(
        "Deriving slice with {} segments for repo {} type {:?}",
        params.segments.len(),
        params.repo_id,
        derived_data_type,
    );

    let manager = get_throttled_manager(repo.repo().repo_derived_data().manager())
        .map_err(AsyncRequestsError::internal)?;
    let manager = with_type_enabled(&manager, derived_data_type);

    let segment_concurrency = justknobs::get_as::<i64>(JK_BACKFILL_SEGMENT_CONCURRENCY, None)
        .map_err(AsyncRequestsError::internal)? as usize;
    let segment_chunk_size = justknobs::get_as::<i64>(JK_BACKFILL_SEGMENT_CHUNK_SIZE, None)
        .map_err(AsyncRequestsError::internal)? as usize;

    // Derive each segment by explicitly enumerating all changesets from base
    // to head, then deriving them in batches.
    let commit_graph = repo.repo().commit_graph();
    let derived_count = Arc::new(AtomicUsize::new(0));

    // Parse segments upfront so we can iterate concurrently.
    let segments: Vec<(ChangesetId, ChangesetId)> = params
        .segments
        .iter()
        .map(|seg| {
            let base = ChangesetId::from_bytes(&seg.base)?;
            let head = ChangesetId::from_bytes(&seg.head)?;
            anyhow::Ok((base, head))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(AsyncRequestsError::request)?;

    stream::iter(segments)
        .map(Ok::<_, Error>)
        .try_for_each_concurrent(Some(segment_concurrency), |(base, head)| {
            let manager = manager.clone();
            let ctx = ctx.clone();
            let commit_graph = commit_graph.clone();
            let derived_count = derived_count.clone();
            async move {
                let segment_cs_ids: Vec<ChangesetId> = commit_graph
                    .range_stream(&ctx, base, head)
                    .await?
                    .collect()
                    .await;

                for chunk in segment_cs_ids.chunks(segment_chunk_size) {
                    BulkDerivation::derive_exactly_underived_batch(
                        &manager,
                        &ctx,
                        chunk,
                        None,
                        derived_data_type,
                    )
                    .await?;
                }
                derived_count.fetch_add(segment_cs_ids.len(), Ordering::SeqCst);
                Ok::<_, Error>(())
            }
        })
        .await
        .map_err(AsyncRequestsError::internal)?;

    let derived_count = derived_count.load(Ordering::SeqCst) as i64;

    Ok(thrift::DeriveSliceResponse {
        derived_count,
        error_message: None,
        ..Default::default()
    })
}

/// Compute derive_backfill_repo request - computes slices and boundaries for a single repo and enqueues sub-requests.
pub(crate) async fn compute_derive_backfill_repo(
    ctx: &CoreContext,
    mononoke: Arc<Mononoke<Repo>>,
    queue: &AsyncMethodRequestQueue,
    params: thrift::DeriveBackfillRepoParams,
    root_request_id: RowId,
) -> Result<thrift::DeriveBackfillRepoResponse, AsyncRequestsError> {
    let repo_id = RepositoryId::new(
        params
            .repo_id
            .try_into()
            .map_err(|e| AsyncRequestsError::request(anyhow::anyhow!("Invalid repo_id: {}", e)))?,
    );

    let repo = mononoke
        .repo_by_id(ctx.clone(), repo_id)
        .await
        .map_err(AsyncRequestsError::internal)?
        .ok_or_else(|| {
            AsyncRequestsError::request(anyhow::anyhow!("Repo not found: {}", params.repo_id))
        })?
        .with_authorization_context(AuthorizationContext::new_bypass_access_control())
        .build()
        .await
        .map_err(AsyncRequestsError::internal)?;

    let cs_ids: Vec<ChangesetId> = params
        .cs_ids
        .iter()
        .map(ChangesetId::from_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(AsyncRequestsError::request)?;

    let derived_data_type =
        DerivableType::from_name(&params.derived_data_type).map_err(AsyncRequestsError::request)?;

    let slice_size = params.slice_size.max(1) as u64;

    let total_sub_requests = process_repo_backfill(
        ctx,
        &repo,
        queue,
        derived_data_type,
        cs_ids,
        slice_size,
        params.rederive,
        params.reslice,
        params.boundaries_concurrency,
        params.config_name.as_deref(),
        &repo_id,
        &root_request_id,
    )
    .await?;

    Ok(thrift::DeriveBackfillRepoResponse {
        total_sub_requests,
        error_message: None,
        ..Default::default()
    })
}

/// Handles a DeriveBackfill request by iterating over repo_entries
/// and enqueueing a DeriveBackfillRepo sub-request for each repo.
pub(crate) async fn compute_derive_backfill(
    ctx: &CoreContext,
    _mononoke: Arc<Mononoke<Repo>>,
    queue: &AsyncMethodRequestQueue,
    params: thrift::DeriveBackfillParams,
    root_request_id: RowId,
) -> Result<thrift::DeriveBackfillResponse, AsyncRequestsError> {
    if params.repo_entries.is_empty() {
        return Err(AsyncRequestsError::request(anyhow::anyhow!(
            "repo_entries must not be empty"
        )));
    }

    // Validate derived_data_type upfront
    DerivableType::from_name(&params.derived_data_type).map_err(AsyncRequestsError::request)?;

    let total_sub_requests = params.repo_entries.len() as i64;

    stream::iter(&params.repo_entries)
        .map(Ok::<_, AsyncRequestsError>)
        .try_for_each_concurrent(Some(1000), |entry| {
            let ctx = ctx.clone();
            let derived_data_type = params.derived_data_type.clone();
            let config_name = params.config_name.clone();
            let slice_size = params.slice_size;
            let boundaries_concurrency = params.boundaries_concurrency;
            let rederive = params.rederive;
            let reslice = params.reslice;
            let entry_repo_id = entry.repo_id;
            let cs_ids = entry.cs_ids.clone();
            let root_request_id = root_request_id.clone();
            async move {
                let repo_id = RepositoryId::new(entry_repo_id.try_into().map_err(|e| {
                    AsyncRequestsError::request(anyhow::anyhow!("Invalid repo_id: {}", e))
                })?);

                let repo_params = thrift::DeriveBackfillRepoParams {
                    repo_id: entry_repo_id,
                    derived_data_type,
                    cs_ids,
                    slice_size,
                    boundaries_concurrency,
                    rederive,
                    config_name,
                    reslice,
                    ..Default::default()
                };

                queue
                    .enqueue_with_root(&ctx, Some(&repo_id), repo_params, &root_request_id)
                    .await
                    .map_err(AsyncRequestsError::internal)?;

                Ok(())
            }
        })
        .await?;

    info!(
        "DeriveBackfill enqueued {} DeriveBackfillRepo sub-requests across {} repos",
        total_sub_requests,
        params.repo_entries.len(),
    );

    Ok(thrift::DeriveBackfillResponse {
        total_sub_requests,
        error_message: None,
        ..Default::default()
    })
}

/// Process backfill for a single repo: filter underived changesets, compute
/// slices, and enqueue boundary/slice sub-requests.
/// Returns the number of sub-requests enqueued for this repo.
async fn process_repo_backfill(
    ctx: &CoreContext,
    repo: &RepoContext<Repo>,
    queue: &AsyncMethodRequestQueue,
    derived_data_type: DerivableType,
    cs_ids: Vec<ChangesetId>,
    slice_size: u64,
    rederive: bool,
    reslice: bool,
    boundaries_concurrency: i32,
    config_name: Option<&str>,
    repo_id: &RepositoryId,
    root_request_id: &RowId,
) -> Result<i64, AsyncRequestsError> {
    let inner_repo = repo.repo();
    let manager = if let Some(config_name) = config_name {
        inner_repo
            .repo_derived_data()
            .manager_for_config(config_name)
            .map_err(AsyncRequestsError::request)?
    } else {
        inner_repo.repo_derived_data().manager()
    };
    let manager = with_type_enabled(manager, derived_data_type);

    info!(
        "DeriveBackfill for repo {} type {:?}: {} changesets, slice_size {}",
        repo_id.id(),
        derived_data_type,
        cs_ids.len(),
        slice_size,
    );

    // rederive implies reslice: if re-deriving everything, slices must cover
    // all ancestors too.
    let skip_filtering = reslice || rederive;

    // Filter to only underived changesets (unless skip_filtering is set)
    let mut cs_ids = cs_ids;
    if !skip_filtering {
        cs_ids = manager
            .pending(ctx, &cs_ids, None, derived_data_type)
            .await
            .map_err(AsyncRequestsError::internal)?;
        if cs_ids.is_empty() {
            info!(
                "All changesets already derived for repo {}, nothing to enqueue",
                repo_id.id()
            );
            return Ok(0);
        }
        info!("{} changesets still underived", cs_ids.len());
    }

    // Find the derived frontier
    let excluded_ancestors = if skip_filtering {
        vec![]
    } else {
        let (frontier_stats, frontier) = inner_repo
            .commit_graph()
            .ancestors_frontier_with(ctx, cs_ids.clone(), |cs_id| {
                let manager = manager.clone();
                async move {
                    Ok(manager
                        .is_derived(ctx, cs_id, None, derived_data_type)
                        .await?)
                }
            })
            .try_timed()
            .await
            .map_err(AsyncRequestsError::internal)?;
        info!(
            "Computed derived frontier ({} changesets) in {}ms",
            frontier.len(),
            frontier_stats.completion_time.as_millis(),
        );
        frontier
    };

    // Compute segmented slices and boundary changesets
    let (slices_stats, (slices, boundary_changesets)) = inner_repo
        .commit_graph()
        .segmented_slice_ancestors(ctx, cs_ids, excluded_ancestors, slice_size)
        .try_timed()
        .await
        .map_err(AsyncRequestsError::internal)?;
    info!(
        "Computed {} slices with {} boundary changesets in {}ms",
        slices.len(),
        boundary_changesets.len(),
        slices_stats.completion_time.as_millis(),
    );

    if slices.is_empty() && boundary_changesets.is_empty() {
        info!("Nothing to enqueue for repo {}", repo_id.id());
        return Ok(0);
    }

    let serial_slices = requires_serial_slice_processing(derived_data_type);

    // For parallel types, enqueue boundary derivation so slices can
    // proceed independently. Serial types ignore boundaries.
    let boundary_row_id = if !serial_slices && !boundary_changesets.is_empty() {
        let boundary_cs_bytes: Vec<Vec<u8>> = boundary_changesets
            .iter()
            .map(|cs_id| cs_id.as_ref().to_vec())
            .collect();

        let boundary_params = thrift::DeriveBoundariesParams {
            repo_id: repo_id.id() as i64,
            derived_data_type: derived_data_type.name().to_string(),
            boundary_cs_ids: boundary_cs_bytes,
            concurrency: boundaries_concurrency,
            use_predecessor_derivation: true,
            ..Default::default()
        };

        let boundary_token = queue
            .enqueue_with_root(ctx, Some(repo_id), boundary_params, root_request_id)
            .await
            .map_err(AsyncRequestsError::internal)?;
        let id = boundary_token.id();
        info!(
            "Enqueued boundary derivation request (id={}, {} changesets)",
            id.0,
            boundary_changesets.len(),
        );
        Some(id)
    } else {
        None
    };

    // Enqueue slice derivation requests.
    // Serial types: each slice depends on the previous (topological order).
    // Parallel types: all slices depend on the boundary request.
    let mut prev_slice_row_id: Option<RowId> = None;
    for (i, slice) in slices.iter().enumerate() {
        let segments: Vec<thrift::DeriveSliceSegment> = slice
            .segments
            .iter()
            .map(|seg| thrift::DeriveSliceSegment {
                head: seg.head.as_ref().to_vec(),
                base: seg.base.as_ref().to_vec(),
                ..Default::default()
            })
            .collect();

        let slice_params = thrift::DeriveSliceParams {
            repo_id: repo_id.id() as i64,
            derived_data_type: derived_data_type.name().to_string(),
            segments,
            ..Default::default()
        };

        let depends_on: Vec<RowId> = if serial_slices {
            prev_slice_row_id.iter().cloned().collect()
        } else {
            boundary_row_id.iter().cloned().collect()
        };

        let slice_token = queue
            .enqueue_with_dependencies_and_root(
                ctx,
                Some(repo_id),
                slice_params,
                &depends_on,
                root_request_id,
            )
            .await
            .map_err(AsyncRequestsError::internal)?;
        let slice_row_id = slice_token.id();
        info!(
            "Enqueued slice {}/{} (id={}, {} segments)",
            i + 1,
            slices.len(),
            slice_row_id.0,
            slice.segments.len(),
        );

        if serial_slices {
            prev_slice_row_id = Some(slice_row_id);
        }
    }

    let boundary_count = if boundary_row_id.is_some() { 1i64 } else { 0 };
    let total = boundary_count + slices.len() as i64;
    info!(
        "Enqueued {} sub-requests for repo {} ({} boundary + {} slices)",
        total,
        repo_id.id(),
        boundary_count,
        slices.len(),
    );

    Ok(total)
}
