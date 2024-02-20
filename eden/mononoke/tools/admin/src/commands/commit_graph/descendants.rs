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
pub struct DescendantsArgs {
    /// Commit IDs to display descendants of.
    #[clap(long, use_value_delimiter = true)]
    cs_ids: Vec<String>,
}

pub async fn descendants(ctx: &CoreContext, repo: &Repo, args: DescendantsArgs) -> Result<()> {
    let cs_ids: Vec<_> = try_join_all(
        args.cs_ids
            .iter()
            .map(|id| parse_commit_id(ctx, repo, id))
            .collect::<Vec<_>>(),
    )
    .await?;

    let descendants = repo.commit_graph().descendants(ctx, cs_ids).await?;

    for descendant in descendants {
        println!("{}", descendant);
    }

    Ok(())
}
