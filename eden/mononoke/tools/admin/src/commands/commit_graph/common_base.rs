/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use clap::Args;
use commit_graph::CommitGraphRef;
use commit_id::parse_commit_id;
use context::CoreContext;
use futures::future::try_join_all;

use super::Repo;

#[derive(Args)]
pub struct CommonBaseArgs {
    /// The ids of the two commits to calculate the common base for.
    #[clap(long, short = 'i')]
    cs_ids: Vec<String>,
}

pub async fn common_base(ctx: &CoreContext, repo: &Repo, args: CommonBaseArgs) -> Result<()> {
    let cs_ids: Vec<_> = try_join_all(
        args.cs_ids
            .iter()
            .map(|id| parse_commit_id(ctx, repo, id))
            .collect::<Vec<_>>(),
    )
    .await?;

    if cs_ids.len() != 2 {
        bail!("Must provide exactly two commit IDs.");
    }

    let common_base_cs_ids = repo
        .commit_graph()
        .common_base(ctx, cs_ids[0], cs_ids[1])
        .await?;

    for cs_id in common_base_cs_ids {
        println!("{}", cs_id);
    }

    Ok(())
}
