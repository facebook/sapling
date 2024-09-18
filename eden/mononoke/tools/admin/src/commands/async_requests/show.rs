/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_requests::types::AsynchronousRequestParams;
use async_requests::types::AsynchronousRequestResult;
use async_requests::types::IntoConfigFormat;
use async_requests::types::RowId;
use async_requests::types::ThriftAsynchronousRequestParams;
use async_requests::types::ThriftAsynchronousRequestResult;
use async_requests::types::ThriftMegarepoSyncChangesetResult;
use clap::Args;
use client::AsyncRequestsQueue;
use context::CoreContext;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_types::ChangesetId;

#[derive(Args)]
/// Subcommand responsible for showing the request
/// details.
pub struct AsyncRequestsShowArgs {
    /// ID of the request.
    #[clap(long)]
    request_id: u64,
}

struct ParamsWrapper<'a, R>(&'a Mononoke<R>, AsynchronousRequestParams);

impl<'a, R: MononokeRepo> std::fmt::Debug for ParamsWrapper<'a, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // impl Debug for HexArray here
        match self.1.thrift() {
            ThriftAsynchronousRequestParams::megarepo_add_target_params(params) => f
                .debug_struct("MegarepoAddTargetParams")
                .field(
                    "config_with_new_target",
                    &params
                        .config_with_new_target
                        .clone()
                        .into_config_format(self.0),
                )
                .field(
                    "changesets_to_merge",
                    &params
                        .changesets_to_merge
                        .iter()
                        .map(|(k, v)| (k.clone(), ChangesetId::from_bytes(v)))
                        .collect::<Vec<_>>(),
                )
                .field("message", &params.message)
                .finish()?,
            ThriftAsynchronousRequestParams::megarepo_change_target_params(params) => f
                .debug_struct("MegarepoAddTargetParams")
                .field("target", &params.target)
                .field("new_version", &params.new_version)
                .field(
                    "target_location",
                    &ChangesetId::from_bytes(&params.target_location),
                )
                .field(
                    "changesets_to_merge",
                    &params
                        .changesets_to_merge
                        .iter()
                        .map(|(k, v)| (k.clone(), ChangesetId::from_bytes(v)))
                        .collect::<Vec<_>>(),
                )
                .field("message", &params.message)
                .finish()?,
            ThriftAsynchronousRequestParams::megarepo_sync_changeset_params(params) => f
                .debug_struct("MegarepoSyncChangesetParams")
                .field("source_name", &params.source_name)
                .field("target", &params.target)
                .field("cs_id", &ChangesetId::from_bytes(&params.cs_id))
                .field(
                    "target_location",
                    &ChangesetId::from_bytes(&params.target_location),
                )
                .finish()?,
            other => f.write_str(format!("{:?}", other).as_str())?,
        }
        Ok(())
    }
}

struct ResultsWrapper(Option<AsynchronousRequestResult>);
impl std::fmt::Debug for ResultsWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // impl Debug for HexArray here
        match &self.0 {
            Some(res) => match res.thrift() {
                ThriftAsynchronousRequestResult::megarepo_sync_changeset_result(
                    ThriftMegarepoSyncChangesetResult::success(result),
                ) => f
                    .debug_struct("MegarepoSyncChangesetResponse")
                    .field("cs_id", &ChangesetId::from_bytes(&result.cs_id))
                    .finish()?,
                other => f.write_str(format!("{:?}", other).as_str())?,
            },
            None => (),
        }
        Ok(())
    }
}

pub async fn show_request<R: MononokeRepo>(
    args: AsyncRequestsShowArgs,
    ctx: CoreContext,
    queues_client: AsyncRequestsQueue,
    mononoke: Arc<Mononoke<R>>,
) -> Result<(), Error> {
    let queue = queues_client
        .async_method_request_queue(&ctx)
        .await
        .context("obtaining async queue")?;

    let row_id = args.request_id;

    if let Some((_request_id, entry, params, maybe_result)) = queue
        .get_request_by_id(&ctx, &RowId(row_id))
        .await
        .context("retrieving the request")?
    {
        println!(
            "Entry: {:#?}\nParams: {:#?}\nResult: {:#?}",
            entry,
            ParamsWrapper(&mononoke, params),
            ResultsWrapper(maybe_result),
        );
        Ok(())
    } else {
        Err(anyhow!("Request not found."))
    }
}
