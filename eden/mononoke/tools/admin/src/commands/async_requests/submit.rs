/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_requests::types::ThriftMegarepoAddBranchingTargetParams;
use async_requests::types::ThriftMegarepoAddTargetParams;
use async_requests::types::ThriftMegarepoChangeTargetConfigParams;
use async_requests::types::ThriftMegarepoRemergeSourceParams;
use async_requests::types::ThriftMegarepoSyncChangesetParams;
use async_requests::types::ThriftParams;
use async_requests::AsyncMethodRequestQueue;
use clap::Args;
use client::AsyncRequestsQueue;
use context::CoreContext;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;

#[derive(Args)]
/// Changes the request status to ready and put error as result.
/// (this won't stop any currently running workers immediately)
pub struct AsyncRequestsSubmitArgs {
    /// The method name for the request.
    #[clap(long, short)]
    method: String,

    /// The request params as a JSON file.
    #[clap(long, short)]
    params: String,
}

pub async fn submit_request<R: MononokeRepo>(
    args: AsyncRequestsSubmitArgs,
    ctx: CoreContext,
    queues_client: AsyncRequestsQueue,
    mononoke: Arc<Mononoke<R>>,
    _repo: R,
) -> Result<(), Error> {
    let queue = queues_client
        .async_method_request_queue(&ctx)
        .await
        .context("obtaining all async queues")?;

    let params = fs::read_to_string(args.params)?;
    match args.method.as_str() {
        "megarepo_add_sync_target" => {
            let params: ThriftMegarepoAddTargetParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoAddTargetParams, R>(&ctx, mononoke, queue, params).await
        }
        "megarepo_add_branching_sync_target" => {
            let params: ThriftMegarepoAddBranchingTargetParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoAddBranchingTargetParams, R>(&ctx, mononoke, queue, params)
                .await
        }
        "megarepo_change_target_config" => {
            let params: ThriftMegarepoChangeTargetConfigParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoChangeTargetConfigParams, R>(&ctx, mononoke, queue, params)
                .await
        }
        "megarepo_sync_changeset" => {
            let params: ThriftMegarepoSyncChangesetParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoSyncChangesetParams, R>(&ctx, mononoke, queue, params).await
        }
        "megarepo_remerge_source" => {
            let params: ThriftMegarepoRemergeSourceParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoRemergeSourceParams, R>(&ctx, mononoke, queue, params).await
        }
        _ => bail!("method {} not supported in submit", args.method),
    }?;

    Ok(())
}

async fn enqueue<P: ThriftParams, R: MononokeRepo>(
    ctx: &CoreContext,
    mononoke: Arc<Mononoke<R>>,
    queue: AsyncMethodRequestQueue,
    params: P,
) -> Result<()> {
    let _token = queue
        .enqueue(ctx, &mononoke, params)
        .await
        .context("updating the request")?;
    Ok(())
}
