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
use async_requests::types::RowId;
use clap::Args;
use context::CoreContext;
use mononoke_api::MononokeRepo;

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
    queue: AsyncMethodRequestQueue,
) -> Result<(), Error> {
    let row_id = args.request_id;

    if let Some((request_id, _entry, _params, _maybe_result)) = queue
        .get_request_by_id(&ctx, &RowId(row_id))
        .await
        .context("retrieving the request")?
    {
        queue
            .requeue(&ctx, request_id)
            .await
            .context("requeuing the request")?;
        Ok(())
    } else {
        Err(anyhow!("Request not found."))
    }
}
