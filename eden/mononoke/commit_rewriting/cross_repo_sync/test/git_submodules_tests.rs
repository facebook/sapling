/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_snake_case)]

//! Tests for handling git submodules in x-repo sync

use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use git_types::MappedGitCommitId;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_derived_data::RepoDerivedDataRef;
use tests_utils::CreateCommitContext;

use crate::check_mapping;
use crate::git_submodules_test_utils::*;
use crate::sync_to_master;

/**!
 * Test submodule expansion when syncing a small repo with submodule changes
 * to a large repo.
 *
 * These tests use repo A as the small repo, depending on repo B as a submodule.
 */

/// Tests the basic setup of expanding a submodule.
/// Also test that adding and deleting files in the submodule repo will generate
/// the proper change in its expansion.
#[fbinit::test]
async fn test_submodule_expansion_basic(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        repo_b_info: (repo_b, repo_b_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(fb).await?;

    // Check mappings from base commits
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        *repo_a_cs_map.get("A_A").unwrap(),
        ChangesetId::from_str("28b30c7f04abbba1c636e7cc36d9181685a45a35a903b69074631d135c873869")
            .ok(),
    )
    .await;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        *repo_a_cs_map.get("A_B").unwrap(),
        ChangesetId::from_str("beeb0fbae2b578f4db9c2413da2d3e9c925abb6cce7ffb7aaee16b26b685100f")
            .ok(),
    )
    .await;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        *repo_a_cs_map.get("A_C").unwrap(),
        ChangesetId::from_str("dd3b9ad41f6984993899d4d7dee27bae1cc01bbbd3f481646cf7f6a67638e369")
            .ok(),
    )
    .await;

    // Modify repo_b, update submodule pointer in repo_a, sync this commit
    // to large repo and check that submodule expansion was updated properly

    let repo_b_cs_id =
        CreateCommitContext::new(&ctx, &repo_b, vec![*repo_b_cs_map.get("B_B").unwrap()])
            .set_message("Add and delete file from repo_b")
            .add_file("new_dir/new_file", "new file content")
            .delete_file("B_B")
            .commit()
            .await?;

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_id)
        .await?;
    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    const MESSAGE: &str = "Update submodule after adding and deleting a file";

    let repo_a_cs_id =
        CreateCommitContext::new(&ctx, &repo_a, vec![*repo_a_cs_map.get("A_C").unwrap()])
            .set_message(MESSAGE)
            .add_file_with_type(
                "submodules/repo_b",
                repo_b_git_commit_hash.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let _large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, repo_a_cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;

    let expected_cs_id =
        ChangesetId::from_str("eb139e09af89e542b3d0d272857e7400ba721e814e3a06d94c85dfcea8e0c124")
            .unwrap();

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        repo_a_cs_id,
        Some(expected_cs_id),
    )
    .await;

    compare_expected_changesets_from_basic_setup(
        large_repo_changesets,
        vec![ExpectedChangeset::new_by_file_change(
            MESSAGE,
            // File changes only contain exact delta and change to submodule
            // metadata file
            vec![
                // Submodule metadata file is updated
                "repo_a/submodules/.x-repo-submodule-repo_b",
                "repo_a/submodules/repo_b/new_dir/new_file",
            ],
            // File was deleted
            vec!["repo_a/submodules/repo_b/B_B"],
        )],
    )?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        expected_cs_id,
        NonRootMPath::new("repo_a/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    Ok(())
}
