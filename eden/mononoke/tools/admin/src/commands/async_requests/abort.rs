/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use megarepo_api::MegarepoApi;

use async_requests::types::MegarepoAsynchronousRequestResult;
use async_requests::types::RowId;
use async_requests::types::ThriftMegarepoAsynchronousRequestParams;
use megarepo_error::MegarepoError;
use source_control::MegarepoAddBranchingTargetResult;
use source_control::MegarepoAddTargetResult;
use source_control::MegarepoChangeTargetConfigResult;
use source_control::MegarepoRemergeSourceResult;
use source_control::MegarepoSyncChangesetResult;

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
    megarepo: MegarepoApi,
) -> Result<(), Error> {
    let repos_and_queues = megarepo.all_async_method_request_queues(&ctx).await?;

    let row_id = args.request_id;

    for (_repo_ids, queue) in repos_and_queues {
        if let Some((request_id, _entry, params, maybe_result)) =
            queue.get_request_by_id(&ctx, &RowId(row_id)).await?
        {
            if maybe_result == None {
                let err = MegarepoError::InternalError(anyhow!("aborted from CLI!").into());
                let result: MegarepoAsynchronousRequestResult  = match params.thrift() {
                    ThriftMegarepoAsynchronousRequestParams::megarepo_sync_changeset_params(_) => {
                        MegarepoSyncChangesetResult::error(err.into()).into()
                    }
                    ThriftMegarepoAsynchronousRequestParams::megarepo_add_target_params(_) => {
                        MegarepoAddTargetResult::error(err.into()).into()
                    }
                    ThriftMegarepoAsynchronousRequestParams::megarepo_change_target_params(_) => {
                        MegarepoChangeTargetConfigResult::error(err.into()).into()
                    }
                    ThriftMegarepoAsynchronousRequestParams::megarepo_remerge_source_params(_) => {
                        MegarepoRemergeSourceResult::error(err.into()).into()
                    }
                    ThriftMegarepoAsynchronousRequestParams::megarepo_add_branching_target_params(_) => {
                        MegarepoAddBranchingTargetResult::error(err.into()).into()
                    }
                    _ => return Err(anyhow!("unknown request type!"))
                };
                queue.complete(&ctx, &request_id, result).await?;
            } else {
                return Err(anyhow!("Request already completed."));
            }
            return Ok(());
        }
    }
    Err(anyhow!("Request not found."))
}
