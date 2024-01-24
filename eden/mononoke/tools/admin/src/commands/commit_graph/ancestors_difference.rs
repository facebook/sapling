/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use commit_graph::CommitGraphRef;
use commit_id::parse_commit_id;
use context::CoreContext;
use futures::future::try_join_all;
use futures::StreamExt;

use super::Repo;

#[derive(Args)]
pub struct AncestorsDifferenceArgs {
    /// Commit IDs to display ancestors of.
    #[clap(long, use_value_delimiter = true)]
    heads: Vec<String>,

    /// Commit IDs to exclude ancestors of.
    #[clap(long, use_value_delimiter = true)]
    common: Vec<String>,
}

pub async fn ancestors_difference(
    ctx: &CoreContext,
    repo: &Repo,
    args: AncestorsDifferenceArgs,
) -> Result<()> {
    let heads: Vec<_> = try_join_all(
        args.heads
            .iter()
            .map(|id| parse_commit_id(ctx, repo, id))
            .collect::<Vec<_>>(),
    )
    .await?;
    let common: Vec<_> = try_join_all(
        args.common
            .iter()
            .map(|id| parse_commit_id(ctx, repo, id))
            .collect::<Vec<_>>(),
    )
    .await?;

    let mut ancestors_difference_stream = Box::pin(
        repo.commit_graph()
            .ancestors_difference_stream(ctx, heads, common)
            .await?,
    );
    while let Some(ancestor_result) = ancestors_difference_stream.next().await {
        println!("{}", ancestor_result?);
    }

    Ok(())
}
