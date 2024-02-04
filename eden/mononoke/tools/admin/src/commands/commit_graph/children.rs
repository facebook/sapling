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

use super::Repo;

#[derive(Args)]
pub struct ChildrenArgs {
    /// Commit ID to display children of.
    #[clap(long, short = 'i')]
    cs_id: String,
}

pub async fn children(ctx: &CoreContext, repo: &Repo, args: ChildrenArgs) -> Result<()> {
    let cs_id = parse_commit_id(ctx, repo, &args.cs_id).await?;

    let children = repo.commit_graph().changeset_children(ctx, cs_id).await?;

    for child in children {
        println!("{}", child);
    }

    Ok(())
}
