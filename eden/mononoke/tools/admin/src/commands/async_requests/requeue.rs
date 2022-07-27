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

use async_requests::types::RowId;
use megarepo_api::MegarepoApi;

#[derive(Args)]
/// Changes the request status to ready and put error as result.
/// (this won't stop any currently running workers immediately)
pub struct AsyncRequestsRequeueArgs {
    /// ID of the request.
    #[clap(long)]
    request_id: u64,
}

pub async fn requeue_request(
    args: AsyncRequestsRequeueArgs,
    ctx: CoreContext,
    megarepo: MegarepoApi,
) -> Result<(), Error> {
    let repos_and_queues = megarepo.all_async_method_request_queues(&ctx).await?;

    let row_id = args.request_id;

    for (_repo_ids, queue) in repos_and_queues {
        if let Some((request_id, _entry, _params, _maybe_result)) =
            queue.get_request_by_id(&ctx, &RowId(row_id)).await?
        {
            queue.requeue(&ctx, request_id).await?;
            return Ok(());
        }
    }
    Err(anyhow!("Request not found."))
}
