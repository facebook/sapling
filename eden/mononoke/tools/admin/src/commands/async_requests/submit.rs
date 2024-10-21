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
use async_requests::types::ThriftParams;
use async_requests::types::Token;
use async_requests::AsyncMethodRequestQueue;
use clap::Args;
use context::CoreContext;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api::RepositoryId;
use repo_identity::RepoIdentityRef;
use source_control as thrift;

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
    queue: AsyncMethodRequestQueue,
    repo: Repo,
) -> Result<(), Error> {
    let repo_id = repo.repo_identity().id();

    let params = fs::read_to_string(args.params)?;
    let token = match args.method.as_str() {
        "megarepo_add_sync_target" => {
            let params: thrift::MegarepoAddTargetParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<thrift::MegarepoAddTargetParams>(&ctx, queue, Some(&repo_id), params).await
        }
        "megarepo_add_branching_sync_target" => {
            let params: thrift::MegarepoAddBranchingTargetParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<thrift::MegarepoAddBranchingTargetParams>(&ctx, queue, Some(&repo_id), params)
                .await
        }
        "megarepo_change_target_config" => {
            let params: thrift::MegarepoChangeTargetConfigParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<thrift::MegarepoChangeTargetConfigParams>(&ctx, queue, Some(&repo_id), params)
                .await
        }
        "megarepo_sync_changeset" => {
            let params: thrift::MegarepoSyncChangesetParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<thrift::MegarepoSyncChangesetParams>(&ctx, queue, Some(&repo_id), params)
                .await
        }
        "megarepo_remerge_source" => {
            let params: thrift::MegarepoRemergeSourceParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<thrift::MegarepoRemergeSourceParams>(&ctx, queue, Some(&repo_id), params)
                .await
        }
        "ping" => {
            let params: thrift::AsyncPingParams =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<thrift::AsyncPingParams>(&ctx, queue, None, params).await
        }
        "commit_sparse_profile_size" => {
            let params: thrift::CommitSparseProfileSizeParamsV2 =
                serde_json::from_str(&params).context("parsing params")?;
            enqueue::<thrift::CommitSparseProfileSizeParamsV2>(&ctx, queue, Some(&repo_id), params)
                .await
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
