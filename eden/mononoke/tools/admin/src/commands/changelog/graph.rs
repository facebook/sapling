/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;

use anyhow::Result;
use clap::Args;
use commit_id::parse_commit_id;
use context::CoreContext;
use futures::future::join_all;
use mononoke_types::ChangesetId;
use print_graph::print_graph;
use print_graph::PrintGraphOptions;

use super::Repo;

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

    /// Display commit's file changes
    #[clap(long, short = 'F')]
    pub display_file_changes: bool,
}

pub async fn graph(ctx: &CoreContext, repo: &Repo, graph_args: ChangelogGraphArgs) -> Result<()> {
    let changesets = join_all(
        graph_args
            .changesets
            .iter()
            .map(move |head| async move { parse_commit_id(ctx, repo, head).await }),
    )
    .await
    .into_iter()
    .collect::<Result<Vec<ChangesetId>>>()?;

    let print_graph_args = PrintGraphOptions {
        limit: graph_args.limit,
        display_message: graph_args.display_message,
        display_id: graph_args.display_id,
        display_author: graph_args.display_author,
        display_author_date: graph_args.display_author_date,
        display_file_changes: graph_args.display_file_changes,
    };

    let writer = Box::new(io::stdout());
    print_graph(ctx, repo, changesets, print_graph_args, writer).await
}
