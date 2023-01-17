/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use changeset_fetcher::ChangesetFetcherArc;
use clap::Args;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use mononoke_types::ChangesetId;
use smallvec::ToSmallVec;

use super::Repo;

#[derive(Args)]
pub struct BackfillOneArgs {
    /// Commit ID to backfill (bonsai)
    #[clap(long, short = 'i')]
    commit_id: ChangesetId,
}

pub(super) async fn backfill_one(
    ctx: &CoreContext,
    repo: &Repo,
    args: BackfillOneArgs,
) -> Result<()> {
    let changeset_fetcher = repo.changeset_fetcher_arc();
    let parents = changeset_fetcher
        .get_parents(ctx, args.commit_id)
        .await?
        .to_smallvec();

    let (stats, result) = repo
        .commit_graph()
        .add_recursive(ctx, changeset_fetcher, args.commit_id, parents)
        .timed()
        .await;

    println!("Finished backfilling in {:?}", stats);
    println!("{} changesets added to commit graph", result?);

    Ok(())
}
