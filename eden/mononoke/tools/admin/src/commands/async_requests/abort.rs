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
        ThriftAsynchronousRequestParams::async_ping_params(_) => default_err,
        ThriftAsynchronousRequestParams::derive_boundaries_params(_) => default_err,
        ThriftAsynchronousRequestParams::derive_slice_params(_) => default_err,
        ThriftAsynchronousRequestParams::derive_backfill_params(_) => default_err,
        ThriftAsynchronousRequestParams::derive_backfill_repo_params(_) => default_err,
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

pub async fn abort_request(
    args: AsyncRequestsAbortArgs,
    ctx: CoreContext,
    queue: AsyncMethodRequestQueue,
) -> Result<(), Error> {
    if let Some(root_id) = args.root_request_id {
        let root_row_id = RowId(root_id);

        // Batch-fail all 'new' requests with this root.
        let failed_count = queue
            .fail_new_requests_by_root_id(&ctx, &root_row_id)
            .await
            .context("batch-failing new requests")?;
        println!("{} pending requests failed", failed_count);

        // Get remaining requests for this root to abort in-progress ones.
        let entries = queue
            .get_requests_by_root_id(&ctx, &root_row_id)
            .await
            .context("fetching requests by root id")?;

        let mut aborted = 0u64;
        let mut skipped = 0u64;
        for (request_id, entry, params, maybe_result) in entries {
            if maybe_result.is_some() || entry.status != RequestStatus::InProgress {
                skipped += 1;
                continue;
            }

            if abort_single_request(&ctx, &queue, &request_id, &params).await? {
                aborted += 1;
            } else {
                // Request was completed by a worker between our SELECT and UPDATE.
                skipped += 1;
            }
        }

        println!(
            "{} in-progress requests aborted, {} already completed (skipped)",
            aborted, skipped
        );
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
