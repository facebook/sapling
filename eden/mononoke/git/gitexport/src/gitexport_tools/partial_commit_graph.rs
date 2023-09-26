/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use futures::future::try_join_all;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::stream::{self};
use itertools::Itertools;
use mononoke_api::changeset_path::ChangesetPathHistoryContext;
use mononoke_api::changeset_path::ChangesetPathHistoryOptions;
use mononoke_api::ChangesetContext;
use mononoke_api::MononokeError;
use mononoke_api::MononokePath;
use mononoke_types::ChangesetId;
use slog::debug;
use slog::info;
use slog::trace;
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
    // Consider history until the provided timestamp, i.e. all commits in the
    // graph will have its creation time greater than or equal to it.
    oldest_commit_ts: Option<i64>,
) -> Result<PartialGraphInfo, MononokeError>
where
    P: TryInto<MononokePath>,
    MononokeError: From<P::Error>,
{
    info!(logger, "Building partial commit graph for export...");

    let cs_path_hist_ctxs: Vec<ChangesetPathHistoryContext> = stream::iter(paths)
        .then(|p| async { cs_ctx.path_with_history(p).await })
        .try_collect::<Vec<_>>()
        .await?;

    let cs_path_history_options = ChangesetPathHistoryOptions {
        follow_history_across_deletions: true,
        until_timestamp: oldest_commit_ts,
        ..Default::default()
    };

    // Get each path's history as a vector of changesets
    let history_changesets: Vec<Vec<ChangesetContext>> = try_join_all(
        try_join_all(
            cs_path_hist_ctxs
                .iter()
                .map(|csphc| csphc.history(cs_path_history_options)),
        )
        .await?
        .into_iter()
        .map(|stream| stream.try_collect()),
    )
    .await?;

    let (sorted_changesets, parents_map) =
        merge_cs_lists_and_build_parents_map(logger, history_changesets).await?;

    info!(
        logger,
        "Number of changsets to export: {0:?}",
        sorted_changesets.len()
    );

    // TODO(gustavoavena): remove these prints for debugging after adding tests
    let cs_msgs: Vec<_> = try_join_all(sorted_changesets.iter().map(|csc| csc.message())).await?;
    trace!(logger, "changeset messages: {0:#?}", cs_msgs);

    info!(logger, "Partial commit graph built!");
    Ok((sorted_changesets, parents_map))
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
) -> Result<(Vec<ChangesetContext>, ChangesetParents), Error> {
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

#[cfg(test)]
#[allow(non_snake_case)] // For test commits
mod test {

    use std::str::FromStr;

    use fbinit::FacebookInit;
    use mononoke_api::BookmarkFreshness;
    use mononoke_api::BookmarkKey;
    use mononoke_api::CoreContext;
    use mononoke_types::NonRootMPath;
    use slog_glog_fmt::logger_that_can_work_in_tests;
    use test_utils::build_test_repo;
    use test_utils::GitExportTestRepoOptions;
    use test_utils::EXPORT_DIR;
    use test_utils::SECOND_EXPORT_DIR;

    use super::*;

    #[fbinit::test]
    async fn test_partial_commit_graph_for_single_export_path(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let logger = logger_that_can_work_in_tests().unwrap();

        let export_dir = NonRootMPath::new(EXPORT_DIR).unwrap();

        let (source_repo_ctx, changeset_ids) =
            build_test_repo(fb, &ctx, GitExportTestRepoOptions::default()).await?;

        let A = changeset_ids["A"];
        let C = changeset_ids["C"];
        let E = changeset_ids["E"];
        let G = changeset_ids["G"];
        let I = changeset_ids["I"];
        let J = changeset_ids["J"];

        // Ids of the changesets that are expected to be rewritten
        let expected_cs_ids: Vec<ChangesetId> = vec![A, C, E, G, I, J];

        let expected_parent_map = HashMap::from([
            (A, vec![]),
            (C, vec![A]),
            (E, vec![C]),
            (G, vec![E]),
            (I, vec![G]),
            (J, vec![I]),
        ]);

        let master_cs = source_repo_ctx
            .resolve_bookmark(
                &BookmarkKey::from_str("master")?,
                BookmarkFreshness::MostRecent,
            )
            .await?
            .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

        let (relevant_css, parents_map) =
            build_partial_commit_graph_for_export(&logger, vec![export_dir], master_cs, None)
                .await?;

        let relevant_cs_ids = relevant_css
            .iter()
            .map(ChangesetContext::id)
            .collect::<Vec<_>>();

        assert_eq!(expected_cs_ids, relevant_cs_ids);

        assert_eq!(expected_parent_map, parents_map);

        Ok(())
    }

    #[fbinit::test]
    async fn test_directories_with_merge_commits_fail_hard(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let logger = logger_that_can_work_in_tests().unwrap();

        let export_dir = NonRootMPath::new(EXPORT_DIR).unwrap();
        let second_export_dir = NonRootMPath::new(SECOND_EXPORT_DIR).unwrap();
        let test_repo_opts = GitExportTestRepoOptions {
            add_branch_commit: true,
        };

        let (source_repo_ctx, changeset_ids) = build_test_repo(fb, &ctx, test_repo_opts).await?;

        let F = changeset_ids["F"];
        let branch_commit = changeset_ids["K"];

        let master_cs = source_repo_ctx
            .resolve_bookmark(
                &BookmarkKey::from_str("master")?,
                BookmarkFreshness::MostRecent,
            )
            .await?
            .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

        let error = build_partial_commit_graph_for_export(
            &logger,
            vec![export_dir, second_export_dir],
            master_cs,
            None,
        )
        .await
        .unwrap_err();

        let expected_error = format!(
            "internal error: Merge commits are not supported for partial commit graphs. Commit {0:?} is not an ancestor of {1:?}",
            branch_commit, F
        );
        assert_eq!(expected_error, error.to_string(),);

        Ok(())
    }

    #[fbinit::test]
    async fn test_partial_commit_graph_for_multiple_export_paths(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let logger = logger_that_can_work_in_tests().unwrap();

        let export_dir = NonRootMPath::new(EXPORT_DIR).unwrap();
        let second_export_dir = NonRootMPath::new(SECOND_EXPORT_DIR).unwrap();

        let test_repo_opts = GitExportTestRepoOptions {
            add_branch_commit: false,
        };
        let (source_repo_ctx, changeset_ids) = build_test_repo(fb, &ctx, test_repo_opts).await?;

        let A = changeset_ids["A"];
        let C = changeset_ids["C"];
        let E = changeset_ids["E"];
        // The F commit changes only the file in the second export path
        let F = changeset_ids["F"];
        let G = changeset_ids["G"];
        let I = changeset_ids["I"];
        let J = changeset_ids["J"];

        // Ids of the changesets that are expected to be rewritten
        let expected_cs_ids: Vec<ChangesetId> = vec![A, C, E, F, G, I, J];

        let expected_parent_map = HashMap::from([
            (A, vec![]),
            (C, vec![A]),
            (E, vec![C]),
            (F, vec![E]),
            (G, vec![F]),
            (I, vec![G]),
            (J, vec![I]),
        ]);

        let master_cs = source_repo_ctx
            .resolve_bookmark(
                &BookmarkKey::from_str("master")?,
                BookmarkFreshness::MostRecent,
            )
            .await?
            .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

        let (relevant_css, parents_map) = build_partial_commit_graph_for_export(
            &logger,
            vec![export_dir, second_export_dir],
            master_cs,
            None,
        )
        .await?;

        let relevant_cs_ids = relevant_css
            .iter()
            .map(ChangesetContext::id)
            .collect::<Vec<_>>();

        assert_eq!(expected_cs_ids, relevant_cs_ids);

        assert_eq!(expected_parent_map, parents_map);

        Ok(())
    }

    #[fbinit::test]
    async fn test_oldest_commit_ts_option(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let logger = logger_that_can_work_in_tests().unwrap();

        let export_dir = NonRootMPath::new(EXPORT_DIR).unwrap();
        let second_export_dir = NonRootMPath::new(SECOND_EXPORT_DIR).unwrap();

        let test_repo_opts = GitExportTestRepoOptions {
            add_branch_commit: false,
        };
        let (source_repo_ctx, changeset_ids) = build_test_repo(fb, &ctx, test_repo_opts).await?;

        let E = changeset_ids["E"];
        // The F commit changes only the file in the second export path
        let F = changeset_ids["F"];
        let G = changeset_ids["G"];
        let I = changeset_ids["I"];
        let J = changeset_ids["J"];

        // Ids of the changesets that are expected to be rewritten
        // Ids of the changesets that are expected to be rewritten.
        // First and C commits would also be included, but we're going to
        // use E's author date as the oldest_commit_ts argument.
        let expected_cs_ids: Vec<ChangesetId> = vec![E, F, G, I, J];

        let expected_parent_map = HashMap::from([
            (E, vec![]),
            (F, vec![E]),
            (G, vec![F]),
            (I, vec![G]),
            (J, vec![I]),
        ]);

        let master_cs = source_repo_ctx
            .resolve_bookmark(
                &BookmarkKey::from_str("master")?,
                BookmarkFreshness::MostRecent,
            )
            .await?
            .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

        let fifth_cs = source_repo_ctx
            .changeset(E)
            .await?
            .ok_or(anyhow!("Failed to get changeset context of E commit"))?;

        let oldest_ts = fifth_cs.author_date().await?.timestamp();

        let (relevant_css, parents_map) = build_partial_commit_graph_for_export(
            &logger,
            vec![export_dir, second_export_dir],
            master_cs,
            Some(oldest_ts),
        )
        .await?;

        let relevant_cs_ids = relevant_css
            .iter()
            .map(ChangesetContext::id)
            .collect::<Vec<_>>();

        assert_eq!(expected_cs_ids, relevant_cs_ids);

        assert_eq!(expected_parent_map, parents_map);

        Ok(())
    }
}
