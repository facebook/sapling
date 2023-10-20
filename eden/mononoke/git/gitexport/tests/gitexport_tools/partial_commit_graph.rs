/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(test)]
#![allow(non_snake_case)] // For test commits

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Result;
use borrowed::borrowed;
use fbinit::FacebookInit;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::stream::{self};
use gitexport_tools::build_partial_commit_graph_for_export;
use gitexport_tools::MASTER_BOOKMARK;
use maplit::hashmap;
use mononoke_api::BookmarkFreshness;
use mononoke_api::BookmarkKey;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use slog::info;
use slog_glog_fmt::logger_that_can_work_in_tests;
use test_utils::build_test_repo;
use test_utils::repo_with_multiple_renamed_export_directories;
use test_utils::repo_with_renamed_export_path;
use test_utils::GitExportTestRepoOptions;

#[fbinit::test]
async fn test_partial_commit_graph_for_single_export_path(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let logger = logger_that_can_work_in_tests().unwrap();

    let test_data = build_test_repo(fb, &ctx, GitExportTestRepoOptions::default()).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();

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
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let graph_info =
        build_partial_commit_graph_for_export(&logger, vec![(export_dir, master_cs)], None).await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    assert_eq!(expected_cs_ids, relevant_cs_ids);

    assert_eq!(expected_parent_map, graph_info.parents_map);

    Ok(())
}

#[fbinit::test]
async fn test_directories_with_merge_commits_fail_hard(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let logger = logger_that_can_work_in_tests().unwrap();

    let test_repo_opts = GitExportTestRepoOptions {
        add_branch_commit: true,
    };

    let test_data = build_test_repo(fb, &ctx, test_repo_opts).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();
    let second_export_dir = NonRootMPath::new(relevant_paths["second_export_dir"]).unwrap();

    let F = changeset_ids["F"];
    let branch_commit = changeset_ids["K"];

    let master_cs = source_repo_ctx
        .resolve_bookmark(
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let error = build_partial_commit_graph_for_export(
        &logger,
        vec![
            (export_dir, master_cs.clone()),
            (second_export_dir, master_cs),
        ],
        None,
    )
    .await
    .unwrap_err();

    let expected_error = format!(
        "Merge commits are not supported for partial commit graphs. Commit {0:?} is not an ancestor of {1:?}",
        branch_commit, F
    );
    assert_eq!(expected_error, error.to_string(),);

    Ok(())
}

#[fbinit::test]
async fn test_partial_commit_graph_for_multiple_export_paths(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let logger = logger_that_can_work_in_tests().unwrap();

    let test_repo_opts = GitExportTestRepoOptions {
        add_branch_commit: false,
    };

    let test_data = build_test_repo(fb, &ctx, test_repo_opts).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();
    let second_export_dir = NonRootMPath::new(relevant_paths["second_export_dir"]).unwrap();

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
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let graph_info = build_partial_commit_graph_for_export(
        &logger,
        vec![
            (export_dir, master_cs.clone()),
            (second_export_dir, master_cs),
        ],
        None,
    )
    .await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    assert_eq!(expected_cs_ids, relevant_cs_ids);

    assert_eq!(expected_parent_map, graph_info.parents_map);

    Ok(())
}

#[fbinit::test]
async fn test_oldest_commit_ts_option(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let logger = logger_that_can_work_in_tests().unwrap();

    let test_repo_opts = GitExportTestRepoOptions {
        add_branch_commit: false,
    };

    let test_data = build_test_repo(fb, &ctx, test_repo_opts).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let export_dir = NonRootMPath::new(relevant_paths["export_dir"]).unwrap();
    let second_export_dir = NonRootMPath::new(relevant_paths["second_export_dir"]).unwrap();

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
            &BookmarkKey::from_str(MASTER_BOOKMARK)?,
            BookmarkFreshness::MostRecent,
        )
        .await?
        .ok_or(anyhow!("Couldn't find master bookmark in source repo."))?;

    let fifth_cs = source_repo_ctx
        .changeset(E)
        .await?
        .ok_or(anyhow!("Failed to get changeset context of E commit"))?;

    let oldest_ts = fifth_cs.author_date().await?.timestamp();

    let graph_info = build_partial_commit_graph_for_export(
        &logger,
        vec![
            (export_dir, master_cs.clone()),
            (second_export_dir, master_cs),
        ],
        Some(oldest_ts),
    )
    .await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    assert_eq!(expected_cs_ids, relevant_cs_ids);

    assert_eq!(expected_parent_map, graph_info.parents_map);

    Ok(())
}

/// Test different scenarios using a history where the export path was renamed
/// throughout history.
/// The rename won't be automatically followed, but the user can provide
/// the export path's old name and the changeset where the rename happened so
/// history is followed without exporting what we don't want.
///
/// NOTE: changesets are passed as string slices and they're ids and changeset
/// contexts are fetched after the test repo is built.
async fn test_renamed_export_paths_are_followed(
    source_repo_ctx: RepoContext,
    changeset_ids: BTreeMap<String, ChangesetId>,
    // Path and the name of its upper bounds changeset
    export_paths: Vec<(NonRootMPath, &str)>,
    expected_relevant_changesets: Vec<&str>,
    expected_parent_map: HashMap<&str, Vec<&str>>,
) -> Result<()> {
    let logger = logger_that_can_work_in_tests().unwrap();

    info!(
        logger,
        "Testing renamed export paths witht the following paths {0:#?}", export_paths
    );

    let expected_cs_ids: Vec<ChangesetId> = expected_relevant_changesets
        .into_iter()
        .map(|cs_name| changeset_ids[cs_name])
        .collect();

    let expected_parent_map: HashMap<ChangesetId, Vec<ChangesetId>> = expected_parent_map
        .into_iter()
        .map(|(cs_name, parent_names)| {
            let parent_ids: Vec<ChangesetId> =
                parent_names.into_iter().map(|p| changeset_ids[p]).collect();
            (changeset_ids[cs_name], parent_ids)
        })
        .collect();

    let export_path_infos: Vec<(NonRootMPath, ChangesetContext)> = stream::iter(export_paths)
        .then(|(path, cs_name): (NonRootMPath, &str)| {
            borrowed!(changeset_ids);
            borrowed!(source_repo_ctx);
            async move {
                let cs_id = changeset_ids[cs_name];
                let cs_context = source_repo_ctx.changeset(cs_id).await?.ok_or(anyhow!(
                    "Failed to fetch changeset context from commit {}.",
                    cs_name
                ))?;

                anyhow::Ok::<(NonRootMPath, ChangesetContext)>((path, cs_context))
            }
        })
        .try_collect::<Vec<(NonRootMPath, ChangesetContext)>>()
        .await?;

    let graph_info =
        build_partial_commit_graph_for_export(&logger, export_path_infos, None).await?;

    let relevant_cs_ids = graph_info
        .changesets
        .iter()
        .map(ChangesetContext::id)
        .collect::<Vec<_>>();

    assert_eq!(expected_cs_ids, relevant_cs_ids);

    assert_eq!(expected_parent_map, graph_info.parents_map);

    Ok(())
}

/// When user manually specifies the old name of an export path along with
/// the commit where the rename happened as this paths head, the commit history
/// should be followed.
#[fbinit::test]
async fn test_renamed_export_paths_are_followed_manually_passing_old(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let test_data = repo_with_renamed_export_path(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let old_export_dir = NonRootMPath::new(relevant_paths["old_export_dir"]).unwrap();
    let new_export_dir = NonRootMPath::new(relevant_paths["new_export_dir"]).unwrap();
    let head_id: &str = test_data.head_id;

    // Passing the old name of the export path manually
    test_renamed_export_paths_are_followed(
        source_repo_ctx,
        changeset_ids,
        vec![(new_export_dir.clone(), head_id), (old_export_dir, "E")],
        vec!["A", "C", "E", "F", head_id],
        hashmap! {
            "A" => vec![],
            "C" => vec!["A"],
            "E" => vec!["C"],
            "F" => vec!["E"],
            head_id => vec!["F"],
        },
    )
    .await
}

#[fbinit::test]
async fn test_renamed_export_paths_are_not_followed_automatically(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let test_data = repo_with_renamed_export_path(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;

    let new_export_dir = NonRootMPath::new(relevant_paths["new_export_dir"]).unwrap();
    let head_id: &str = test_data.head_id;

    test_renamed_export_paths_are_followed(
        source_repo_ctx,
        changeset_ids,
        vec![(new_export_dir.clone(), head_id)],
        vec!["E", "F", head_id],
        hashmap! {
            "E" => vec![],
            "F" => vec!["E"],
            head_id => vec!["F"],
        },
    )
    .await
}

#[fbinit::test]
async fn test_partial_graph_with_two_renamed_export_directories(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let test_data = repo_with_multiple_renamed_export_directories(fb, &ctx).await?;
    let source_repo_ctx = test_data.repo_ctx;
    let changeset_ids = test_data.commit_id_map;
    let relevant_paths = test_data.relevant_paths;
    let head_id: &str = test_data.head_id;

    let new_bar = NonRootMPath::new(relevant_paths["new_bar_dir"]).unwrap();
    let new_foo = NonRootMPath::new(relevant_paths["new_foo_dir"]).unwrap();

    test_renamed_export_paths_are_followed(
        source_repo_ctx,
        changeset_ids,
        vec![(new_bar.clone(), head_id), (new_foo.clone(), head_id)],
        vec!["B", "D"], // Expected relevant commits
        hashmap! {
            "B" => vec![],
            "D" => vec!["B"],
        },
    )
    .await
}
