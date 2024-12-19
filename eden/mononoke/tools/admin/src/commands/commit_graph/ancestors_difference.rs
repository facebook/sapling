/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use borrowed::borrowed;
use clap::Args;
use commit_graph::CommitGraphRef;
use commit_id::parse_commit_id;
use context::CoreContext;
use futures::future;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use renderdag::Ancestor;
use renderdag::GraphRowRenderer;
use renderdag::Renderer;

use super::Repo;

#[derive(Args)]
pub struct AncestorsDifferenceArgs {
    /// Commit IDs to display ancestors of.
    #[clap(long, use_value_delimiter = true)]
    heads: Vec<String>,

    /// Commit IDs to exclude ancestors of.
    #[clap(long, use_value_delimiter = true)]
    common: Vec<String>,

    /// Render the commits as a graph.
    #[clap(long, short = 'G')]
    graph: bool,
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
    let common_set = common.iter().copied().collect::<HashSet<_>>();

    let mut ancestors_difference_stream = Box::pin(
        repo.commit_graph()
            .ancestors_difference_stream(ctx, heads, common)
            .await?,
    );
    if args.graph {
        let mut renderer = GraphRowRenderer::new().output().build_box_drawing();

        ancestors_difference_stream
            .map_ok(|ancestor| {
                borrowed!(common_set);
                async move {
                    let parents = repo.commit_graph().changeset_parents(ctx, ancestor).await?;
                    let parents = stream::iter(parents)
                        .map(|parent| async move {
                            if repo
                                .commit_graph
                                .is_ancestor_of_any(
                                    ctx,
                                    parent,
                                    common_set.iter().copied().collect(),
                                )
                                .await?
                            {
                                // This parent is an ancestor of the common
                                // set, so it will not be shown.
                                anyhow::Ok(Ancestor::Anonymous)
                            } else {
                                anyhow::Ok(Ancestor::Parent(parent.to_string()))
                            }
                        })
                        .buffered(10)
                        .try_collect::<Vec<_>>()
                        .await?;
                    Ok((ancestor.to_string(), parents))
                }
            })
            .try_buffered(1000)
            .try_for_each(|(ancestor, parents)| {
                let row = renderer.next_row(ancestor.clone(), parents, "o".to_string(), ancestor);
                print!("{}", row);
                future::ok(())
            })
            .await?;
    } else {
        while let Some(ancestor_result) = ancestors_difference_stream.next().await {
            println!("{}", ancestor_result?);
        }
    }

    Ok(())
}
