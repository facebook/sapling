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

use async_requests::types::RowId;

#[derive(Args)]
/// Subcommand responsible for showing the request
/// details.
pub struct AsyncRequestsShowArgs {
    /// ID of the request.
    #[clap(long)]
    request_id: u64,
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
                "Entry: {:?}\nParams: {:?}\nResult: {:?}",
                entry, params, maybe_result
            );
            return Ok(());
        }
    }
    Err(anyhow!("Request not found."))
}
