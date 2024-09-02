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
use clap::Args;
use context::CoreContext;
use megarepo_api::MegarepoApi;
use megarepo_error::MegarepoError;
use mononoke_api::MononokeRepo;
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

pub async fn abort_request<R: MononokeRepo>(
    args: AsyncRequestsAbortArgs,
    ctx: CoreContext,
    megarepo: MegarepoApi<R>,
) -> Result<(), Error> {
    let repos_and_queues = megarepo
        .all_async_method_request_queues(&ctx)
        .await
        .context("obtaining all async queues")?;

    let row_id = args.request_id;

    for (_repo_ids, queue) in repos_and_queues {
        if let Some((request_id, _entry, params, maybe_result)) = queue
            .get_request_by_id(&ctx, &RowId(row_id))
            .await
            .context("retrieving the request")?
        {
            if maybe_result.is_none() {
                let err = MegarepoError::InternalError(anyhow!("aborted from CLI!").into());
                let result: AsynchronousRequestResult = match params.thrift() {
                    ThriftAsynchronousRequestParams::megarepo_sync_changeset_params(_) => {
                        MegarepoSyncChangesetResult::error(err.into()).into()
                    }
                    ThriftAsynchronousRequestParams::megarepo_add_target_params(_) => {
                        MegarepoAddTargetResult::error(err.into()).into()
                    }
                    ThriftAsynchronousRequestParams::megarepo_change_target_params(_) => {
                        MegarepoChangeTargetConfigResult::error(err.into()).into()
                    }
                    ThriftAsynchronousRequestParams::megarepo_remerge_source_params(_) => {
                        MegarepoRemergeSourceResult::error(err.into()).into()
                    }
                    ThriftAsynchronousRequestParams::megarepo_add_branching_target_params(_) => {
                        MegarepoAddBranchingTargetResult::error(err.into()).into()
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
            return Ok(());
        }
    }
    Err(anyhow!("Request not found."))
}
