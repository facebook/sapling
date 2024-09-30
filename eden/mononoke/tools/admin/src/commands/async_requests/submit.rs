/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;

use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_requests::types::ThriftAsyncPingParams;
use async_requests::types::ThriftMegarepoAddBranchingTargetParams;
use async_requests::types::ThriftMegarepoAddTargetParams;
use async_requests::types::ThriftMegarepoChangeTargetConfigParams;
use async_requests::types::ThriftMegarepoRemergeSourceParams;
use async_requests::types::ThriftMegarepoSyncChangesetParams;
use async_requests::types::ThriftParams;
use async_requests::types::Token;
use async_requests::AsyncMethodRequestQueue;
use clap::Args;
use client::AsyncRequestsQueue;
use context::CoreContext;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api::RepositoryId;
use repo_identity::RepoIdentityRef;

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

pub async fn submit_request(
    args: AsyncRequestsSubmitArgs,
    ctx: CoreContext,
    queues_client: AsyncRequestsQueue,
    repo: Repo,
) -> Result<(), Error> {
    let queue = queues_client
        .async_method_request_queue(&ctx)
        .await
        .context("obtaining all async queues")?;

    let repo_id = repo.repo_identity().id();

    let params = fs::read_to_string(args.params)?;
    let token = match args.method.as_str() {
        "megarepo_add_sync_target" => {
            let params: ThriftMegarepoAddTargetParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoAddTargetParams>(&ctx, queue, Some(&repo_id), params).await
        }
        "megarepo_add_branching_sync_target" => {
            let params: ThriftMegarepoAddBranchingTargetParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoAddBranchingTargetParams>(&ctx, queue, Some(&repo_id), params)
                .await
        }
        "megarepo_change_target_config" => {
            let params: ThriftMegarepoChangeTargetConfigParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoChangeTargetConfigParams>(&ctx, queue, Some(&repo_id), params)
                .await
        }
        "megarepo_sync_changeset" => {
            let params: ThriftMegarepoSyncChangesetParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoSyncChangesetParams>(&ctx, queue, Some(&repo_id), params).await
        }
        "megarepo_remerge_source" => {
            let params: ThriftMegarepoRemergeSourceParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftMegarepoRemergeSourceParams>(&ctx, queue, Some(&repo_id), params).await
        }
        "ping" => {
            let params: ThriftAsyncPingParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<ThriftAsyncPingParams>(&ctx, queue, Some(&repo_id), params).await
        }
        _ => bail!("method {} not supported in submit", args.method),
    }?;

    println!("Submitted with token: {}", token);

    Ok(())
}

async fn enqueue<P: ThriftParams>(
    ctx: &CoreContext,
    queue: AsyncMethodRequestQueue,
    repo_id: Option<&RepositoryId>,
    params: P,
) -> Result<u64> {
    let token = queue
        .enqueue(ctx, repo_id, params)
        .await
        .context("updating the request")?;
    Ok(token.id().0)
}
