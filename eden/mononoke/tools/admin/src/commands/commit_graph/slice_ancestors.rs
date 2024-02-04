/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use commit_graph::CommitGraphArc;
use commit_graph::CommitGraphRef;
use commit_id::parse_commit_id;
use context::CoreContext;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;

use super::Repo;

#[derive(Args)]
pub struct SliceAncestorsArgs {
    /// IDs of the commits to slice ancestors of.
    #[clap(long, use_value_delimiter = true)]
    heads: Vec<String>,

    /// The size (in generation numbers) of each slice.
    #[clap(long)]
    slice_size: u64,

    /// Minimum generation to stop slicing at.
    #[clap(long)]
    min_generation: u64,
}

pub async fn slice_ancestors(
    ctx: &CoreContext,
    repo: &Repo,
    args: SliceAncestorsArgs,
) -> Result<()> {
    let heads: Vec<_> = try_join_all(
        args.heads
            .iter()
            .map(|id| parse_commit_id(ctx, repo, id))
            .collect::<Vec<_>>(),
    )
    .await?;

    let slices = repo
        .commit_graph()
        .slice_ancestors(
            ctx,
            heads,
            |cs_ids| async {
                stream::iter(cs_ids)
                    .map(|cs_id| async move {
                        anyhow::Ok((
                            cs_id,
                            repo.commit_graph_arc()
                                .changeset_generation(ctx, cs_id)
                                .await?,
                        ))
                    })
                    .buffered(5)
                    .try_filter_map(|(cs_id, gen)| async move {
                        if gen.value() >= args.min_generation {
                            Ok(Some(cs_id))
                        } else {
                            Ok(None)
                        }
                    })
                    .try_collect()
                    .await
            },
            args.slice_size,
        )
        .await?;

    for (gen_group, cs_ids) in slices {
        print!(
            "slice [{}, {}):",
            gen_group.value(),
            gen_group.value() + args.slice_size
        );
        for cs_id in cs_ids {
            print!(" {}", cs_id);
        }
        println!();
    }

    Ok(())
}
