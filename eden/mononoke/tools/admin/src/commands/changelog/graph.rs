/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcherArc;
use clap::Args;
use context::CoreContext;
use dag::render::Ancestor;
use dag::render::GraphRowRenderer;
use dag::render::Renderer;
use futures::future::join_all;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use repo_blobstore::RepoBlobstoreRef;
use std::collections::BinaryHeap;
use std::collections::HashMap;

use super::Repo;
use crate::commit_id::parse_commit_id;

#[derive(Args)]
pub struct ChangelogGraphArgs {
    /// Initial changesets to start displaying from
    #[clap(long, short = 'i', use_value_delimiter = true)]
    changesets: Vec<String>,

    /// Maximum distance from the initial changesets to any displayed changeset
    #[clap(long, short, default_value_t = 10)]
    limit: usize,

    /// Display commit message for all displayed changesets
    #[clap(long, short = 'M')]
    display_message: bool,

    /// Display bonsai id for all displayed changesets
    #[clap(long, short = 'I')]
    display_id: bool,

    /// Display commit author for all displayed changesets
    #[clap(long, short = 'A')]
    display_author: bool,

    /// Display commit author date for all displayed changesets
    #[clap(long, short = 'D')]
    display_author_date: bool,
}

pub fn get_message(graph_args: &ChangelogGraphArgs, cs: BonsaiChangeset) -> String {
    let mut message_vec = Vec::new();

    if graph_args.display_message {
        message_vec.push(format!("message: {}", cs.message()));
    }

    if graph_args.display_id {
        message_vec.push(format!("id: {}", cs.get_changeset_id()));
    }

    if graph_args.display_author {
        message_vec.push(format!("author: {}", cs.author()));
    }

    if graph_args.display_author_date {
        message_vec.push(format!("author date: {}", cs.author_date()));
    }

    message_vec.join(", ")
}

pub async fn graph_changesets(
    ctx: &CoreContext,
    repo: &Repo,
    graph_args: ChangelogGraphArgs,
    changeset_fetcher: ArcChangesetFetcher,
    changesets: Vec<ChangesetId>,
) -> Result<()> {
    let blobstore = repo.repo_blobstore();

    let mut minimum_distance: HashMap<ChangesetId, usize> =
        changesets.iter().cloned().map(|id| (id, 0)).collect();

    let mut to_visit: BinaryHeap<(Generation, ChangesetId)> =
        join_all(changesets.into_iter().map(|head| {
            let ctx = ctx;
            let changeset_fetcher = &changeset_fetcher;
            async move {
                Ok((
                    changeset_fetcher
                        .get_generation_number(ctx.clone(), head)
                        .await?,
                    head,
                ))
            }
        }))
        .await
        .into_iter()
        .collect::<Result<_>>()?;

    let mut renderer = GraphRowRenderer::<ChangesetId>::new()
        .output()
        .build_box_drawing();

    while let Some((_, hash)) = to_visit.pop() {
        let parents = changeset_fetcher.get_parents(ctx.clone(), hash).await?;
        let current_distance = *minimum_distance.get(&hash).unwrap();

        if current_distance > graph_args.limit {
            println!(
                "{}",
                renderer
                    .next_row(hash, Vec::new(), String::from("~"), String::from(""))
                    .trim_end()
            );
            continue;
        }

        let cs = hash
            .load(ctx, blobstore)
            .await
            .with_context(|| format!("Failed to load changeset {}", hash))?;

        println!(
            "{}",
            renderer
                .next_row(
                    hash,
                    parents.iter().cloned().map(Ancestor::Parent).collect(),
                    String::from("o"),
                    get_message(&graph_args, cs),
                )
                .trim_end()
        );

        for parent_id in parents.into_iter() {
            if let Some(&distance) = minimum_distance.get(&parent_id) {
                if current_distance + 1 < distance {
                    minimum_distance.insert(parent_id, current_distance + 1);
                }
            } else {
                let parent_generation = changeset_fetcher
                    .get_generation_number(ctx.clone(), parent_id)
                    .await?;

                minimum_distance.insert(parent_id, current_distance + 1);
                to_visit.push((parent_generation, parent_id));
            }
        }
    }

    Ok(())
}

pub async fn graph(ctx: &CoreContext, repo: &Repo, graph_args: ChangelogGraphArgs) -> Result<()> {
    let changeset_fetcher = repo.changeset_fetcher_arc();

    let mut changesets = join_all(
        graph_args
            .changesets
            .iter()
            .map(move |head| async move { parse_commit_id(ctx, repo, head).await }),
    )
    .await
    .into_iter()
    .collect::<Result<Vec<ChangesetId>>>()?;

    changesets.sort();
    changesets.dedup();

    graph_changesets(ctx, repo, graph_args, changeset_fetcher, changesets).await
}
