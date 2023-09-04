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

    let cs_path_history_options = ChangesetPathHistoryOptions {
        follow_history_across_deletions: true,
        ..Default::default()
    };
    // Get each path's history as a vector of changesets
    let history_changesets: Vec<Vec<ChangesetContext>> = try_join_all(
        try_join_all(
            cs_path_hist_ctxs
                .iter()
                // TODO(T160600443): support other ChangesetPathHistoryOptions
                .map(|csphc| csphc.history(cs_path_history_options)),
        )
        .await?
        .into_iter()
        .map(|stream| stream.try_collect()),
    )
    .await?;

    let (sorted_changesets, parents_map) =
        merge_cs_lists_and_build_parents_map(history_changesets).await?;

    debug!(logger, "sorted_changesets: {0:#?}", &sorted_changesets);

    // TODO(gustavoavena): remove these prints for debugging after adding tests
    let cs_msgs: Vec<_> = try_join_all(sorted_changesets.iter().map(|csc| csc.message())).await?;
    debug!(logger, "changeset messages: {0:#?}", cs_msgs);

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
    changeset_lists: Vec<Vec<ChangesetContext>>,
) -> Result<(Vec<ChangesetContext>, ChangesetParents), Error> {
    let mut changesets_with_gen: Vec<(ChangesetContext, u64)> =
        stream::iter(changeset_lists.into_iter().flatten())
            .then(|cs| async move {
                let generation = cs.generation().await?.value();
                anyhow::Ok((cs, generation))
            })
            .try_collect::<Vec<_>>()
            .await?;

    // Sort by generation number
    changesets_with_gen
        .sort_by(|(cs_a, gen_a), (cs_b, gen_b)| (gen_a, cs_a.id()).cmp(&(gen_b, cs_b.id())));

    // Collect the sorted changesets
    let mut sorted_css = changesets_with_gen
        .into_iter()
        .map(|(cs, _)| cs)
        .collect::<Vec<_>>();

    // Remove any duplicates from the list.
    // NOTE: `dedup_by` can only be used here because the list is sorted!
    sorted_css.dedup_by(|cs_a, cs_b| cs_a.id().eq(&cs_b.id()));

    // Make sure that there are no merge commits by checking that consecutive
    // changesest are ancestors of each other.
    // In this process, also build the parents map.
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

        let first = changeset_ids["first"];
        let third = changeset_ids["third"];
        let fifth = changeset_ids["fifth"];
        let seventh = changeset_ids["seventh"];
        let ninth = changeset_ids["ninth"];
        let tenth = changeset_ids["tenth"];

        // Ids of the changesets that are expected to be rewritten
        let expected_cs_ids: Vec<ChangesetId> = vec![first, third, fifth, seventh, ninth, tenth];

        let expected_parent_map = HashMap::from([
            (first, vec![]),
            (third, vec![first]),
            (fifth, vec![third]),
            (seventh, vec![fifth]),
            (ninth, vec![seventh]),
            (tenth, vec![ninth]),
        ]);

        let master_cs = source_repo_ctx
            .resolve_bookmark(
                &BookmarkKey::from_str("master")?,
                BookmarkFreshness::MostRecent,
            )
            .await?
            .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

        let (relevant_css, parents_map) =
            build_partial_commit_graph_for_export(&logger, vec![export_dir], master_cs).await?;

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

        let sixth = changeset_ids["sixth"];
        let branch_commit = changeset_ids["branch"];

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
        )
        .await
        .unwrap_err();

        let expected_error = format!(
            "internal error: Merge commits are not supported for partial commit graphs. Commit {0:?} is not an ancestor of {1:?}",
            sixth, branch_commit
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

        let first = changeset_ids["first"];
        let third = changeset_ids["third"];
        let fifth = changeset_ids["fifth"];
        // The sixth commit changes only the file in the second export path
        let sixth = changeset_ids["sixth"];
        let seventh = changeset_ids["seventh"];
        let ninth = changeset_ids["ninth"];
        let tenth = changeset_ids["tenth"];

        // Ids of the changesets that are expected to be rewritten
        let expected_cs_ids: Vec<ChangesetId> =
            vec![first, third, fifth, sixth, seventh, ninth, tenth];

        let expected_parent_map = HashMap::from([
            (first, vec![]),
            (third, vec![first]),
            (fifth, vec![third]),
            (sixth, vec![fifth]),
            (seventh, vec![sixth]),
            (ninth, vec![seventh]),
            (tenth, vec![ninth]),
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
