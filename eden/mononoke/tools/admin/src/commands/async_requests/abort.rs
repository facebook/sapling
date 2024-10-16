/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_requests::types::AsynchronousRequestResult;
use async_requests::types::RowId;
use async_requests::types::ThriftAsynchronousRequestParams;
use async_requests::AsyncMethodRequestQueue;
use clap::Args;
use context::CoreContext;
use megarepo_error::MegarepoError;
use mononoke_api::MononokeRepo;
use source_control as thrift;

#[derive(Args)]
/// Changes the request status to ready and put error as result.
/// (this won't stop any currently running workers immediately)
pub struct AsyncRequestsAbortArgs {
    /// ID of the request.
    #[clap(long)]
    request_id: u64,
}

pub async fn abort_request(
    args: AsyncRequestsAbortArgs,
    ctx: CoreContext,
    queue: AsyncMethodRequestQueue,
) -> Result<(), Error> {
    let row_id = args.request_id;

    if let Some((request_id, _entry, params, maybe_result)) = queue
        .get_request_by_id(&ctx, &RowId(row_id))
        .await
        .context("retrieving the request")?
    {
        if maybe_result.is_none() {
            let err = MegarepoError::InternalError(anyhow!("aborted from CLI!").into());
            let result: AsynchronousRequestResult = match params.thrift() {
                ThriftAsynchronousRequestParams::megarepo_sync_changeset_params(_) => {
                    thrift::MegarepoSyncChangesetResult::error(err.into()).into()
                }
                ThriftAsynchronousRequestParams::megarepo_add_target_params(_) => {
                    thrift::MegarepoAddTargetResult::error(err.into()).into()
                }
                ThriftAsynchronousRequestParams::megarepo_change_target_params(_) => {
                    thrift::MegarepoChangeTargetConfigResult::error(err.into()).into()
                }
                ThriftAsynchronousRequestParams::megarepo_remerge_source_params(_) => {
                    thrift::MegarepoRemergeSourceResult::error(err.into()).into()
                }
                ThriftAsynchronousRequestParams::megarepo_add_branching_target_params(_) => {
                    thrift::MegarepoAddBranchingTargetResult::error(err.into()).into()
                }
                _ => return Err(anyhow!("unknown request type!")),
            };
            queue
                .complete(&ctx, &request_id, result)
                .await
                .context("updating the request")?;
        } else {
            return Err(anyhow!("Request already completed."));
        }
        Ok(())
    } else {
        Err(anyhow!("Request not found."))
    }
}
