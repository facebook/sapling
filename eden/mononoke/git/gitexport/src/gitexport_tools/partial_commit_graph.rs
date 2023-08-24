/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use futures::future::try_join_all;
use futures::TryStreamExt;
use itertools::Itertools;
use mononoke_api::changeset_path::ChangesetPathHistoryContext;
use mononoke_api::changeset_path::ChangesetPathHistoryOptions;
use mononoke_api::ChangesetContext;
use mononoke_api::MononokeError;
use mononoke_api::MononokePath;
use mononoke_types::ChangesetId;
use slog::debug;
use slog::Logger;

pub type ChangesetParents = HashMap<ChangesetId, Vec<ChangesetId>>;
pub type PartialGraphInfo = (Vec<ChangesetContext>, ChangesetParents);

/// Given a list of paths and a changeset, return a commit graph
/// containing only commits that are ancestors of the changeset and have
/// modified at least one of the paths.
/// The commit graph is returned as a topologically sorted list of changesets
/// and a hashmap of changset id to their parents' ids.
pub async fn build_partial_commit_graph_for_export<P>(
    logger: &Logger,
    paths: Vec<P>,
    cs_ctx: ChangesetContext,
) -> Result<PartialGraphInfo, MononokeError>
where
    P: TryInto<MononokePath>,
    MononokeError: From<P::Error>,
{
    let mononoke_paths = paths
        .into_iter()
        .map(|path| path.try_into())
        .collect::<Result<Vec<MononokePath>, _>>()?;

    let cs_path_hist_ctxs: Vec<ChangesetPathHistoryContext> = cs_ctx
        .paths_with_history(mononoke_paths.into_iter())
        .await?
        .try_collect()
        .await?;

    // Get each path's history as a vector of changesets
    let history_changesets: Vec<Vec<ChangesetContext>> = try_join_all(
        try_join_all(
            cs_path_hist_ctxs
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

    debug!(logger, "sorted_changesets: {0:#?}", &sorted_changesets);

    // TODO(gustavoavena): remove these prints for debugging after adding tests
    let cs_msgs: Vec<_> = try_join_all(sorted_changesets.iter().map(|csc| csc.message())).await?;
    debug!(logger, "changeset messages: {0:#?}", cs_msgs);

    let parents_map = build_parents_map(&sorted_changesets).await?;

    Ok((sorted_changesets, parents_map))
}

// TODO(T161204758): build parents map during graph traversal.
/// Temporary way to create the parents map for a `PartialGraphInfo` that will
/// be used to export commits in a very simple case (no branches + one path).
async fn build_parents_map(
    // Topologically sorted list of changesets that will be exported.
    changesets: &Vec<ChangesetContext>,
) -> Result<ChangesetParents, MononokeError> {
    let parents = changesets
        .iter()
        .tuple_windows()
        .map(|(parent, child)| (child.id().clone(), vec![parent.id().clone()]))
        .collect::<HashMap<ChangesetId, Vec<ChangesetId>>>();

    // TODO(T161204758): properly sort and dedupe the list of relevant changesets
    Ok(parents)
}

/// Given a list of changeset lists, merge, dedupe and sort them topologically
/// into a single changeset list that can be used to build a commit graph.
fn merge_and_sort_changeset_lists(
    changeset_lists: Vec<Vec<ChangesetContext>>,
) -> Result<Vec<ChangesetContext>, MononokeError> {
    let mut changesets: Vec<ChangesetContext> = changeset_lists.into_iter().flatten().collect();

    // TODO(T161204758): properly sort and dedupe the list of relevant changesets
    // For now, topologically sort the changesets by reverting their order,
    // because the `history` method returns them in reverse topological order.
    changesets.reverse();

    Ok(changesets)
}
