/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_requests::AsyncMethodRequestQueue;
use async_requests::RequestId;
use async_requests::types::AsynchronousRequestParams;
use async_requests::types::AsynchronousRequestResult;
use async_requests::types::RequestStatus;
use async_requests::types::RowId;
use async_requests::types::ThriftAsynchronousRequestParams;
use async_requests::types::ThriftAsynchronousRequestResult;
use clap::Args;
use context::CoreContext;
use megarepo_error::MegarepoError;
use source_control as thrift;
use source_control::AsyncRequestError;

#[derive(Args)]
/// Changes the request status to ready and put error as result.
/// (this won't stop any currently running workers immediately)
pub struct AsyncRequestsAbortArgs {
    /// ID of a single request to abort.
    #[clap(long, required_unless_present = "root_request_id")]
    request_id: Option<u64>,
    /// Abort all requests with this root request ID.
    /// Requests in 'new' status are batch-failed. Requests in 'inprogress'
    /// status are individually aborted with an error result.
    #[clap(
        long,
        required_unless_present = "request_id",
        conflicts_with = "request_id"
    )]
    root_request_id: Option<u64>,
}

/// Abort a single request by writing an error result and marking it ready.
/// Returns true if the request was actually aborted, false if it was
/// already completed by a worker (race condition).
async fn abort_single_request(
    ctx: &CoreContext,
    queue: &AsyncMethodRequestQueue,
    request_id: &RequestId,
    params: &AsynchronousRequestParams,
) -> Result<bool, Error> {
    let megarepo_err = MegarepoError::InternalError(anyhow!("aborted from CLI!").into());
    let default_err =
        AsynchronousRequestResult::from_thrift(ThriftAsynchronousRequestResult::error(
            AsyncRequestError::internal_error(thrift::InternalErrorStruct {
                reason: String::from("aborted from CLI!"),
                backtrace: None,
                source_chain: vec![],
                ..Default::default()
            }),
        ));
    let result: AsynchronousRequestResult = match params.thrift() {
        ThriftAsynchronousRequestParams::megarepo_sync_changeset_params(_) => {
            thrift::MegarepoSyncChangesetResult::error(megarepo_err.into()).into()
        }
        ThriftAsynchronousRequestParams::megarepo_add_target_params(_) => {
            thrift::MegarepoAddTargetResult::error(megarepo_err.into()).into()
        }
        ThriftAsynchronousRequestParams::megarepo_change_target_params(_) => {
            thrift::MegarepoChangeTargetConfigResult::error(megarepo_err.into()).into()
        }
        ThriftAsynchronousRequestParams::megarepo_remerge_source_params(_) => {
            thrift::MegarepoRemergeSourceResult::error(megarepo_err.into()).into()
        }
        ThriftAsynchronousRequestParams::megarepo_add_branching_target_params(_) => {
            thrift::MegarepoAddBranchingTargetResult::error(megarepo_err.into()).into()
        }
        ThriftAsynchronousRequestParams::commit_sparse_profile_size_params(_) => default_err,
        ThriftAsynchronousRequestParams::commit_sparse_profile_delta_params(_) => default_err,
        ThriftAsynchronousRequestParams::derive_boundaries_params(_) => default_err,
        ThriftAsynchronousRequestParams::derive_slice_params(_) => default_err,
        ThriftAsynchronousRequestParams::derive_backfill_params(_) => default_err,
        ThriftAsynchronousRequestParams::derive_backfill_repo_params(_) => default_err,
        ThriftAsynchronousRequestParams::mark_type_enabled_params(_) => default_err,
        ThriftAsynchronousRequestParams::UnknownField(_) => {
            return Err(anyhow!("unknown request type!"));
        }
    };

    let updated = queue
        .complete(ctx, request_id, result)
        .await
        .context("updating the request")?;

    Ok(updated)
}

pub async fn abort_by_root_id(
    ctx: &CoreContext,
    queue: &AsyncMethodRequestQueue,
    root_id: u64,
) -> Result<(), Error> {
    let root_row_id = RowId(root_id);

    // Abort the root request itself first. For a multi-repo backfill scheduler
    // (repo_concurrency > 0) the root is long-lived and keeps scheduling repos;
    // it polls its own status each iteration and stops once it is no longer
    // in-progress, so failing it here is what actually halts further scheduling.
    // Short-lived (fan-out) roots have already completed by the time an abort
    // runs, so the in-progress guard skips them.
    match queue
        .get_request_by_id(ctx, &root_row_id)
        .await
        .context("fetching root request")?
    {
        Some((request_id, entry, params, maybe_result)) => {
            if maybe_result.is_none() && entry.status == RequestStatus::InProgress {
                abort_single_request(ctx, queue, &request_id, &params).await?;
                println!("root request {root_id} aborted");
            }
        }
        None => {
            // `root_id` is used purely as a grouping key for children here; there
            // is no request row with this id (e.g. a synthetic/legacy root id).
            // Nothing to abort at the root -- fall through to the child cleanup.
        }
    }

    // Abort all child requests ('new' -> failed, in-progress -> error result).
    // This is shared with the backfill scheduler (which runs the same cleanup when
    // it notices its root was aborted) so both paths behave identically.
    let (failed_count, aborted, skipped) = queue
        .abort_children_by_root_id(ctx, &root_row_id)
        .await
        .context("aborting child requests")?;
    println!("{failed_count} pending requests failed");
    println!("{aborted} in-progress requests aborted, {skipped} already completed (skipped)");

    Ok(())
}

pub async fn abort_request(
    args: AsyncRequestsAbortArgs,
    ctx: CoreContext,
    queue: AsyncMethodRequestQueue,
) -> Result<(), Error> {
    if let Some(root_id) = args.root_request_id {
        abort_by_root_id(&ctx, &queue, root_id).await?;
    } else if let Some(row_id) = args.request_id {
        if let Some((request_id, _entry, params, maybe_result)) = queue
            .get_request_by_id(&ctx, &RowId(row_id))
            .await
            .context("retrieving the request")?
        {
            if maybe_result.is_none() {
                if !abort_single_request(&ctx, &queue, &request_id, &params).await? {
                    return Err(anyhow!(
                        "Request was completed by a worker before it could be aborted."
                    ));
                }
            } else {
                return Err(anyhow!("Request already completed."));
            }
        } else {
            return Err(anyhow!("Request not found."));
        }
    }

    Ok(())
}
