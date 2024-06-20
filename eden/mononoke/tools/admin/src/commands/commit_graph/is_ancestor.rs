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
use context::PerfCounterType;
use futures::future::try_join_all;
use futures_stats::TimedTryFutureExt;
use slog::debug;

use super::Repo;

#[derive(Args)]
pub struct IsAncestorArgs {
    /// Ids of two commits. The first is the ancestor, and the second is the descendant.
    #[clap(long, short = 'i')]
    cs_ids: Vec<String>,
}

pub async fn is_ancestor(ctx: &CoreContext, repo: &Repo, args: IsAncestorArgs) -> Result<()> {
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

    // Reset perf counters.
    let ctx = ctx.clone_and_reset();

    let (stats, is_ancestor) = repo
        .commit_graph()
        .is_ancestor(&ctx, cs_ids[0], cs_ids[1])
        .try_timed()
        .await?;

    println!("{}", is_ancestor);
    debug!(ctx.logger(), "is-ancestor query finished in {:?}", stats);
    debug!(
        ctx.logger(),
        "sql reads from replicas: {:?}. sql reads from master: {:?}",
        ctx.perf_counters()
            .get_counter(PerfCounterType::SqlReadsReplica),
        ctx.perf_counters()
            .get_counter(PerfCounterType::SqlReadsMaster),
    );

    Ok(())
}
