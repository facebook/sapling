/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Result;
use futures::future::try_join_all;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::stream::{self};
use itertools::Itertools;
use mononoke_api::changeset_path::ChangesetPathHistoryOptions;
use mononoke_api::ChangesetContext;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use slog::debug;
use slog::info;
use slog::Logger;

pub type ChangesetParents = HashMap<ChangesetId, Vec<ChangesetId>>;

/// Represents a path that should be exported until a given changeset, i.e. the
/// HEAD commit for that path.
///
/// When partially copying each relevant changeset to the temporary repo, changes
/// to this path in a given changeset will only be copied if this changeset is
/// an ancestor of the head changeset of that path.
///
/// This head changeset will be used to query the history of the path,
/// i.e. all exported commits that affect this path will be this changeset's
/// ancestor.
pub type ExportPathInfo = (NonRootMPath, ChangesetContext);

#[derive(Debug)]
pub struct GitExportGraphInfo {
    pub changesets: Vec<ChangesetContext>,
    pub parents_map: ChangesetParents,
}

/// Given a list of paths and a changeset, return a commit graph
/// containing only commits that are ancestors of the changeset and have
/// modified at least one of the paths.
/// The commit graph is returned as a topologically sorted list of changesets
/// and a hashmap of changset id to their parents' ids.
pub async fn build_partial_commit_graph_for_export(
    logger: &Logger,
    paths: Vec<ExportPathInfo>,
    // Consider history until the provided timestamp, i.e. all commits in the
    // graph will have its creation time greater than or equal to it.
    oldest_commit_ts: Option<i64>,
) -> Result<GitExportGraphInfo> {
    info!(logger, "Building partial commit graph for export...");

    let cs_path_history_options = ChangesetPathHistoryOptions {
        follow_history_across_deletions: true,
        until_timestamp: oldest_commit_ts,
        ..Default::default()
    };

    let history_changesets: Vec<Vec<ChangesetContext>> = stream::iter(paths)
        .then(|(p, cs_ctx)| async move {
            get_relevant_changesets_for_single_path(p, &cs_ctx, &cs_path_history_options).await
        })
        .try_collect::<Vec<_>>()
        .await?;

    let (sorted_changesets, parents_map) =
        merge_cs_lists_and_build_parents_map(logger, history_changesets).await?;

    info!(
        logger,
        "Number of changsets to export: {0:?}",
        sorted_changesets.len()
    );

    info!(logger, "Partial commit graph built!");
    Ok(GitExportGraphInfo {
        parents_map,
        changesets: sorted_changesets,
    })
}

/// Get all changesets that affected the provided path up to a specific head
/// commit.
async fn get_relevant_changesets_for_single_path(
    path: NonRootMPath,
    head_cs: &ChangesetContext,
    cs_path_history_opts: &ChangesetPathHistoryOptions,
) -> Result<Vec<ChangesetContext>> {
    let cs_path_hist_ctx = head_cs.path_with_history(path).await?;

    let changesets: Vec<ChangesetContext> = cs_path_hist_ctx
        .history(*cs_path_history_opts)
        .await?
        .try_collect()
        .await?;

    Ok(changesets)
}

/// Given a list of changeset lists, merge, dedupe and sort them topologically
/// into a single changeset list that can be used to partially copy commits
/// to a temporary repo.
/// In the process, also build the hashmap containing the parent information
/// **considering only the exported directories**.
///
/// Example: Given the graph `A -> b -> c -> D -> e`, where commits with uppercase
/// have modified export paths, the parent map should be `{A: [D]}`, because
/// the partial graph is `A -> D`.
async fn merge_cs_lists_and_build_parents_map(
    logger: &Logger,
    changeset_lists: Vec<Vec<ChangesetContext>>,
) -> Result<(Vec<ChangesetContext>, ChangesetParents)> {
    info!(
        logger,
        "Merging changeset lists and building parents map..."
    );
    let mut changesets_with_gen: Vec<(ChangesetContext, u64)> =
        stream::iter(changeset_lists.into_iter().flatten())
            .then(|cs| async move {
                let generation = cs.generation().await?.value();
                anyhow::Ok((cs, generation))
            })
            .try_collect::<Vec<_>>()
            .await?;

    // Sort by generation number
    debug!(logger, "Sorting changesets by generation number...");
    changesets_with_gen
        .sort_by(|(cs_a, gen_a), (cs_b, gen_b)| (gen_a, cs_a.id()).cmp(&(gen_b, cs_b.id())));

    // Collect the sorted changesets
    let mut sorted_css = changesets_with_gen
        .into_iter()
        .map(|(cs, _)| cs)
        .collect::<Vec<_>>();

    // Remove any duplicates from the list.
    // NOTE: `dedup_by` can only be used here because the list is sorted!
    debug!(logger, "Deduping changesets...");
    sorted_css.dedup_by(|cs_a, cs_b| cs_a.id().eq(&cs_b.id()));

    // Make sure that there are no merge commits by checking that consecutive
    // changesest are ancestors of each other.
    // In this process, also build the parents map.
    debug!(logger, "Building parents map...");
    let mut parents_map = try_join_all(sorted_css.iter().tuple_windows().map(|(parent, child)| async {
         let is_ancestor = parent.is_ancestor_of(child.id()).await?;
         if !is_ancestor {
             return Err(anyhow!(
                 "Merge commits are not supported for partial commit graphs. Commit {:?} is not an ancestor of {:?}", parent.id(), child.id(),
             ));
         };
         Ok((child.id(), vec![parent.id()]),)
     }))
     .await?
     .into_iter()
     .collect::<HashMap<ChangesetId, Vec<ChangesetId>>>();

    if let Some(root_cs) = sorted_css.first() {
        parents_map.insert(root_cs.id(), vec![]);
    };

    Ok((sorted_css, parents_map))
}
