/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use changeset_fetcher::ChangesetFetcherArc;
use clap::Args;
use context::CoreContext;
use futures::compat::Stream01CompatExt;
use futures::StreamExt;
use futures::TryStreamExt;
use revset::AncestorsNodeStream;

use super::Repo;
use crate::commit_id::parse_commit_id;

#[derive(Args)]
pub struct ChangelogListAncestorsArgs {
    /// Changeset to start at
    #[clap(long, short = 'i')]
    changeset_id: String,

    /// Number of ancestors to fetch
    #[clap(long, short, default_value_t = 10)]
    limit: usize,
}

pub async fn list_ancestors(
    ctx: &CoreContext,
    repo: &Repo,
    list_ancestors_args: ChangelogListAncestorsArgs,
) -> Result<()> {
    let start = parse_commit_id(ctx, repo, &list_ancestors_args.changeset_id).await?;

    let mut ancestors = AncestorsNodeStream::new(ctx.clone(), &repo.changeset_fetcher_arc(), start)
        .compat()
        .take(list_ancestors_args.limit);

    while let Some(cs_id) = ancestors.try_next().await? {
        println!("{}", cs_id);
    }

    Ok(())
}
