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
use commit_id::parse_commit_id;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use smallvec::ToSmallVec;
use vec1::vec1;

use super::Repo;

#[derive(Args)]
pub struct BackfillOneArgs {
    /// Commit ID to backfill
    #[clap(long, short = 'i')]
    commit_id: String,
}

pub(super) async fn backfill_one(
    ctx: &CoreContext,
    repo: &Repo,
    args: BackfillOneArgs,
) -> Result<()> {
    let cs_id = parse_commit_id(ctx, repo, &args.commit_id).await?;

    let changeset_fetcher = repo.changeset_fetcher_arc();
    let parents = changeset_fetcher
        .get_parents(ctx, cs_id)
        .await?
        .to_smallvec();

    let (stats, result) = repo
        .commit_graph()
        .add_recursive(ctx, changeset_fetcher, vec1![(cs_id, parents)])
        .timed()
        .await;

    println!("Finished backfilling in {:?}", stats);
    println!("{} changesets added to commit graph", result?);

    Ok(())
}
