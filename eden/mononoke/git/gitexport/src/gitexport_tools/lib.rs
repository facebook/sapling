/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use commit_graph::CommitGraph;
use futures::future::try_join_all;
use futures::TryStreamExt;
use in_memory_commit_graph_storage::InMemoryCommitGraphStorage;
use mononoke_api::changeset_path::ChangesetPathHistoryContext;
use mononoke_api::changeset_path::ChangesetPathHistoryOptions;
pub use mononoke_api::BookmarkFreshness;
use mononoke_api::ChangesetContext;
use mononoke_api::MononokeError;
use mononoke_api::MononokePath;
use mononoke_types::RepositoryId;

/// Given a list of paths and a changeset, return a commit graph
/// containing only commits that are ancestors of the changeset and have
/// modified at least one of the paths.
pub async fn build_partial_commit_graph_for_export<P>(
    paths: Vec<P>,
    chgset_ctx: ChangesetContext,
) -> Result<CommitGraph, MononokeError>
where
    P: TryInto<MononokePath>,
    MononokeError: From<P::Error>,
{
    let mononoke_paths = paths
        .into_iter()
        .map(|path| path.try_into())
        .collect::<Result<Vec<MononokePath>, _>>()?;

    let chgset_path_hist_ctxs: Vec<ChangesetPathHistoryContext> = chgset_ctx
        .paths_with_history(mononoke_paths.clone().into_iter())
        .await?
        .try_collect()
        .await?;

    // Get each path's history as a vector of changesets
    let history_changesets: Vec<Vec<ChangesetContext>> = try_join_all(
        try_join_all(
            chgset_path_hist_ctxs
                .iter()
                // TODO(T160600443): support other ChangesetPathHistoryOptions
                .map(|csphc| csphc.history(ChangesetPathHistoryOptions::default())),
        )
        .await?
        .into_iter()
        .map(|stream| stream.try_collect()),
    )
    .await?;

    let sorted_changesets = merge_and_sort_changeset_lists(history_changesets)?;
    let stripped_changesets = strip_irrelevant_changes(sorted_changesets, mononoke_paths).await?;

    println!("sorted_changesets: {0:#?}", &stripped_changesets);

    // TODO(gustavoavena): remove these prints for debugging after adding tests
    let chgset_msgs: Vec<_> =
        try_join_all(stripped_changesets.clone().iter().map(|csc| csc.message())).await?;
    println!("chgset_msgs: {0:#?}", chgset_msgs);

    create_commit_graph(stripped_changesets)
}

fn create_commit_graph(_changesets: Vec<ChangesetContext>) -> Result<CommitGraph, MononokeError> {
    let cg_storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

    let commit_graph = CommitGraph::new(cg_storage);

    // TODO(T161204758): add commits to the commit graph

    // TODO(T161204758): properly sort and dedupe the list of relevant changesets
    Ok(commit_graph)
}

/// Given a list of changeset lists, merge, dedupe and sort them topologically
/// into a single changeset list that can be used to build a commit graph.
fn merge_and_sort_changeset_lists(
    changesets: Vec<Vec<ChangesetContext>>,
) -> Result<Vec<ChangesetContext>, MononokeError> {
    // TODO(T161204758): properly sort and dedupe the list of relevant changesets
    Ok(changesets.into_iter().flatten().collect())
}

/// Given a commit graph, create a new graph with every commit stripped of all
/// changes that are not in any of the provided paths.
async fn strip_irrelevant_changes(
    changesets: Vec<ChangesetContext>,
    _paths: Vec<MononokePath>,
) -> Result<Vec<ChangesetContext>, MononokeError> {
    // TODO(T161205476): strip irrelevant changes from a CommitGraph
    Ok(changesets)
}
