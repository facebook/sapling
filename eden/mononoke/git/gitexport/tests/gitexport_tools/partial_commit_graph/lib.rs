/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(test)]
#![allow(non_snake_case)] // For test commits

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use fbinit::FacebookInit;
use gitexport_tools::build_partial_commit_graph_for_export;
use mononoke_api::BookmarkFreshness;
use mononoke_api::BookmarkKey;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use slog_glog_fmt::logger_that_can_work_in_tests;
use test_utils::build_test_repo;
use test_utils::GitExportTestRepoOptions;
use test_utils::EXPORT_DIR;
use test_utils::SECOND_EXPORT_DIR;

#[fbinit::test]
async fn test_partial_commit_graph_for_single_export_path(fb: FacebookInit) -> Result<(), Error> {
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
        build_partial_commit_graph_for_export(&logger, vec![export_dir], master_cs, None).await?;

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
