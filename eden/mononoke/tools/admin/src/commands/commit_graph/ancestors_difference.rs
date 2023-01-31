/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures::future::try_join_all;

use super::Repo;
use crate::commit_id::parse_commit_id;

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

    let ancestors = repo
        .commit_graph()
        .ancestors_difference(ctx, heads, common)
        .await?;

    for cs_id in ancestors {
        println!("{}", cs_id);
    }

    Ok(())
}
