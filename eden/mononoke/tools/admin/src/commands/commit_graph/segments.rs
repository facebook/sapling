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

use super::Repo;

#[derive(Args)]
pub struct SegmentsArgs {
    /// Commit IDs to display ancestors of.
    #[clap(long, use_value_delimiter = true)]
    heads: Vec<String>,

    /// Commit IDs to exclude ancestors of.
    #[clap(long, use_value_delimiter = true)]
    common: Vec<String>,

    /// Verify the correctness of the returned segments using ancestors_difference.
    #[clap(long)]
    verify: bool,
}

pub async fn segments(ctx: &CoreContext, repo: &Repo, args: SegmentsArgs) -> Result<()> {
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

    let segments = match args.verify {
        true => {
            repo.commit_graph()
                .verified_ancestors_difference_segments(ctx, heads, common)
                .await?
        }
        false => {
            repo.commit_graph()
                .ancestors_difference_segments(ctx, heads, common)
                .await?
        }
    };

    for segment in segments {
        println!("{}", segment);
    }

    Ok(())
}
