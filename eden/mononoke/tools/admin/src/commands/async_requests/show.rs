/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_requests::types::IntoConfigFormat;
use async_requests::types::MegarepoAsynchronousRequestParams;
use async_requests::types::MegarepoAsynchronousRequestResult;
use async_requests::types::RowId;
use async_requests::types::ThriftMegarepoAsynchronousRequestParams;
use async_requests::types::ThriftMegarepoAsynchronousRequestResult;
use async_requests::types::ThriftMegarepoSyncChangesetResult;
use clap::Args;
use context::CoreContext;
use megarepo_api::MegarepoApi;
use mononoke_api::Mononoke;
use mononoke_types::ChangesetId;

#[derive(Args)]
/// Subcommand responsible for showing the request
/// details.
pub struct AsyncRequestsShowArgs {
    /// ID of the request.
    #[clap(long)]
    request_id: u64,
}

struct ParamsWrapper<'a>(&'a Mononoke, MegarepoAsynchronousRequestParams);

impl<'a> std::fmt::Debug for ParamsWrapper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // impl Debug for HexArray here
        match self.1.thrift() {
            ThriftMegarepoAsynchronousRequestParams::megarepo_add_target_params(params) => f
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
            ThriftMegarepoAsynchronousRequestParams::megarepo_change_target_params(params) => f
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
            ThriftMegarepoAsynchronousRequestParams::megarepo_sync_changeset_params(params) => f
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

struct ResultsWrapper<'a>(&'a Mononoke, Option<MegarepoAsynchronousRequestResult>);
impl<'a> std::fmt::Debug for ResultsWrapper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // impl Debug for HexArray here
        match &self.1 {
            Some(res) => match res.thrift() {
                ThriftMegarepoAsynchronousRequestResult::megarepo_sync_changeset_result(
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

pub async fn show_request(
    args: AsyncRequestsShowArgs,
    ctx: CoreContext,
    megarepo: MegarepoApi,
) -> Result<(), Error> {
    let repos_and_queues = megarepo.all_async_method_request_queues(&ctx).await?;

    let row_id = args.request_id;

    for (_repo_ids, queue) in repos_and_queues {
        if let Some((_request_id, entry, params, maybe_result)) =
            queue.get_request_by_id(&ctx, &RowId(row_id)).await?
        {
            println!(
                "Entry: {:#?}\nParams: {:#?}\nResult: {:#?}",
                entry,
                ParamsWrapper(&megarepo.mononoke(), params),
                ResultsWrapper(&megarepo.mononoke(), maybe_result),
            );
            return Ok(());
        }
    }
    Err(anyhow!("Request not found."))
}
