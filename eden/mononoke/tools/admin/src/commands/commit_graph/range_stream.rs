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
use futures::try_join;
use futures::StreamExt;

use super::Repo;

#[derive(Args)]
pub struct RangeStreamArgs {
    /// Commit ID of the start of the range.
    #[clap(long)]
    start: String,

    /// Commit ID of the end of the range
    #[clap(long)]
    end: String,
}

pub async fn range_stream(ctx: &CoreContext, repo: &Repo, args: RangeStreamArgs) -> Result<()> {
    let (start, end) = try_join!(
        parse_commit_id(ctx, repo, &args.start),
        parse_commit_id(ctx, repo, &args.end),
    )?;

    let mut range_stream = repo.commit_graph().range_stream(ctx, start, end).await?;

    while let Some(cs_id) = range_stream.next().await {
        println!("{}", cs_id);
    }

    Ok(())
}
