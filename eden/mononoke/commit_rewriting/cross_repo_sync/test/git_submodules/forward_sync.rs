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
use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use context::CoreContext;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use fbinit::FacebookInit;
use maplit::btreemap;
use mononoke_macros::mononoke;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use tests_utils::CreateCommitContext;

use crate::check_mapping;
use crate::git_submodules::git_submodules_test_utils::*;
use crate::sync_to_master;

const REPO_B_SUBMODULE_PATH: &str = "submodules/repo_b";

/**!
 * Test submodule expansion when syncing a small repo with submodule changes
 * to a large repo.
 *
 * These tests use repo A as the small repo, depending on repo B as a submodule.
 */

/// Tests the basic setup of expanding a submodule.
/// Also test that adding and deleting files in the submodule repo will generate
/// the proper change in its expansion.
#[mononoke::fbinit_test]
async fn test_submodule_expansion_basic(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    // Check mappings from base commits
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        *small_repo_cs_map.get("A_A").unwrap(),
        ChangesetId::from_str("8e4c86fbb8af564753141502a96ae89f808b8ff3880b9d8fb82aa33ac055b7d8")
            .ok(),
    )
    .await;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        *small_repo_cs_map.get("A_B").unwrap(),
        ChangesetId::from_str("ff1c511380d99c88b484fa2b0cb742be44f6dca66e85a1c620fcf08454cd6ab6")
            .ok(),
    )
    .await;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_map["A_C"],
        ChangesetId::from_str("8d60517a2c3491ac2cbee5e254153037e9d7c6b83a5ab58a615b841421661bdc")
            .ok(),
    )
    .await;

    // Modify repo_b, update submodule pointer in small_repo, sync this commit
    // to large repo and check that submodule expansion was updated properly

    let repo_b_cs_id =
        CreateCommitContext::new(&ctx, &repo_b, vec![*repo_b_cs_map.get("B_B").unwrap()])
            .set_message("Add and delete file from repo_b")
            .add_file("new_dir/new_file", "new file content")
            .delete_file("B_B")
            .commit()
            .await?;

    let repo_b_git_commit_hash = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id).await?;

    const MESSAGE: &str = "Update submodule after adding and deleting a file";

    let small_repo_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(MESSAGE)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                repo_b_git_commit_hash.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let (large_repo_cs_id, large_repo_changesets) = sync_changeset_and_derive_all_types(
        ctx.clone(),
        small_repo_cs_id,
        &large_repo,
        &commit_syncer,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[ExpectedChangeset::new(MESSAGE)
            .with_regular_changes(
                // File changes only contain exact delta and change to submodule
                // metadata file
                vec![
                    // Submodule metadata file is updated
                    "small_repo/submodules/.x-repo-submodule-repo_b",
                    "small_repo/submodules/repo_b/new_dir/new_file",
                ],
            )
            .with_deletions(
                // File was deleted
                vec!["small_repo/submodules/repo_b/B_B"],
            )],
    )?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    Ok(())
}

/// Tests the basic setup of expanding submodules that contain other submodules.
#[mononoke::fbinit_test]
async fn test_recursive_submodule_expansion_basic(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_git_sha1 = git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"]).await?;

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);
    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            (repo_c_submodule_path, repo_c.clone()),
        ],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    let master_before_change = master_cs_id(&ctx, &large_repo).await?;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        master_before_change,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
            "small_repo/submodules/.x-repo-submodule-repo_b",
            "small_repo/submodules/repo_b/B_A",
            "small_repo/submodules/repo_b/B_B",
            "small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
            "small_repo/submodules/repo_b/submodules/repo_c/C_A",
            "small_repo/submodules/repo_b/submodules/repo_c/C_B",
        ],
    )
    .await?;

    // Modify repo_b, update submodule pointer in small_repo, sync this commit
    // to large repo and check that submodule expansion was updated properly
    let repo_b_cs_id =
        CreateCommitContext::new(&ctx, &repo_b, vec![*repo_b_cs_map.get("B_B").unwrap()])
            .set_message("Add and delete file from repo_b")
            .add_file("new_dir/new_file", "new file content")
            .delete_file("B_B")
            .commit()
            .await?;

    let repo_b_git_commit_hash = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id).await?;

    const MESSAGE: &str = "Update submodule after adding and deleting a file";

    let small_repo_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(MESSAGE)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                repo_b_git_commit_hash.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let (large_repo_cs_id, _large_repo_changesets) = sync_changeset_and_derive_all_types(
        ctx.clone(),
        small_repo_cs_id,
        &large_repo,
        &commit_syncer,
    )
    .await?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c")?,
        &c_master_git_sha1,
    )
    .await?;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
            "small_repo/submodules/.x-repo-submodule-repo_b",
            "small_repo/submodules/repo_b/B_A",
            "small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
            "small_repo/submodules/repo_b/new_dir/new_file",
            "small_repo/submodules/repo_b/submodules/repo_c/C_A",
            "small_repo/submodules/repo_b/submodules/repo_c/C_B",
        ],
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    Ok(())
}

// ------------------------- Deletions ----------------------------

/// Deleting an entire submodule in the small repo (i.e. small_repo) should delete
/// its expansion and its metadata file in small_repo folder in the large repo.
#[mononoke::fbinit_test]
async fn test_submodule_deletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    const MESSAGE: &str = "Delete repo_b submodule in small_repo";
    let cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
        .set_message(MESSAGE)
        .delete_file(REPO_B_SUBMODULE_PATH)
        .commit()
        .await?;

    let (large_repo_cs_id, large_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &large_repo, &commit_syncer)
            .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(large_repo_cs_id)).await;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[ExpectedChangeset::new(MESSAGE)
            // Files being deleted
            .with_deletions(vec![
                "small_repo/submodules/.x-repo-submodule-repo_b",
                "small_repo/submodules/repo_b/B_A",
                "small_repo/submodules/repo_b/B_B",
            ])],
    )?;

    Ok(())
}

/// Test that deleting a recursive submodule also deletes its metadata file.
#[mononoke::fbinit_test]
async fn test_recursive_submodule_deletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let c_master_git_sha1 = git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"]).await?;

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);
    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            (repo_c_submodule_path, repo_c.clone()),
        ],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    // Delete repo_c submodule in repo_b
    let repo_b_cs_id =
        CreateCommitContext::new(&ctx, &repo_b, vec![*repo_b_cs_map.get("B_B").unwrap()])
            .set_message("Add and delete file from repo_b")
            .delete_file(repo_c_submodule_path_in_repo_b)
            .commit()
            .await?;

    let repo_b_git_commit_hash = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id).await?;

    const MESSAGE: &str = "Update submodule after deleting repo_c submodule in repo_b";

    let small_repo_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(MESSAGE)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                repo_b_git_commit_hash.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let (large_repo_cs_id, large_repo_changesets) = sync_changeset_and_derive_all_types(
        ctx.clone(),
        small_repo_cs_id,
        &large_repo,
        &commit_syncer,
    )
    .await?;

    compare_expected_changesets(
        large_repo_changesets.last_chunk::<1>().unwrap(),
        &[ExpectedChangeset::new(MESSAGE)
            .with_regular_changes(
                // repo_b submodule metadata file is updated
                vec!["small_repo/submodules/.x-repo-submodule-repo_b"],
            )
            .with_deletions(
                // Files being deleted
                vec![
                    // NOTE: repo_c submodule metadata file has to be deleted too
                    "small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                    "small_repo/submodules/repo_b/submodules/repo_c/C_A",
                    "small_repo/submodules/repo_b/submodules/repo_c/C_B",
                ],
            )],
    )?;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
            "small_repo/submodules/.x-repo-submodule-repo_b",
            "small_repo/submodules/repo_b/B_A",
            "small_repo/submodules/repo_b/B_B",
        ],
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    Ok(())
}

/// Test that deleting a submodule with a recursive submodule properly deletes
/// both and their metadata files.
#[mononoke::fbinit_test]
async fn test_submodule_with_recursive_submodule_deletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_git_sha1 = git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"]).await?;

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, _repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);
    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            (repo_c_submodule_path, repo_c.clone()),
        ],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    const MESSAGE: &str = "Delete repo_b submodule";

    let small_repo_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(MESSAGE)
            .delete_file(REPO_B_SUBMODULE_PATH)
            .commit()
            .await?;

    let (large_repo_cs_id, large_repo_changesets) = sync_changeset_and_derive_all_types(
        ctx.clone(),
        small_repo_cs_id,
        &large_repo,
        &commit_syncer,
    )
    .await?;

    compare_expected_changesets(
        large_repo_changesets.last_chunk::<1>().unwrap(),
        &[ExpectedChangeset::new(MESSAGE).with_deletions(
            // Files being deleted
            vec![
                "small_repo/submodules/.x-repo-submodule-repo_b",
                "small_repo/submodules/repo_b/B_A",
                "small_repo/submodules/repo_b/B_B",
                // NOTE: repo_c submodule metadata file has to be deleted too
                "small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                "small_repo/submodules/repo_b/submodules/repo_c/C_A",
                "small_repo/submodules/repo_b/submodules/repo_c/C_B",
            ],
        )],
    )?;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
        ],
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    Ok(())
}

/// Test a scenario where users stop using a repository as submodule but keep
/// its contents as static copies in the same path.
/// This should be a valid bonsai, where the removal of the submodule metadata
/// file means that the GitSubmodule file type should be deleted in the small
/// repo when backsyncing.
///
/// This also tests that **later modifying this static copy** also passes
/// validation, even if the path is still in the small repo config as one
/// of its submodule deps.
#[mononoke::fbinit_test]
async fn test_deleting_submodule_but_keeping_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    const DELETE_METADATA_FILE_MSG: &str = "Delete repo_b submodule and kept its static copy";

    let del_md_file_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(DELETE_METADATA_FILE_MSG)
            .delete_file(REPO_B_SUBMODULE_PATH)
            // Delete the submodule file change, but keep the contents in the same path
            .add_file(
                format!("{}/B_A", REPO_B_SUBMODULE_PATH).as_str(),
                "first commit in submodule B",
            )
            .add_file(
                format!("{}/B_B", REPO_B_SUBMODULE_PATH).as_str(),
                "second commit in submodule B",
            )
            .commit()
            .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, del_md_file_cs_id)
        .await
        .context("sync_to_master failed")
        .and_then(|res| res.ok_or(anyhow!("No commit was synced")))?;

    let first_expected_cs_id = large_repo_cs_id;

    println!("large_repo_cs_id: {0:#?}", large_repo_cs_id);

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
            // Files from the submodule are now regular files in the small repo
            "small_repo/submodules/repo_b/B_A",
            "small_repo/submodules/repo_b/B_B",
        ],
    )
    .await?;

    const CHANGE_SUBMODULE_PATH_MSG: &str = "Change static copy of repo_b";

    let chg_sm_path_cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![del_md_file_cs_id])
        .set_message(CHANGE_SUBMODULE_PATH_MSG)
        // Modify files in the submodule path, because they're now regular files
        // in the small repo
        .delete_file(format!("{}/B_A", REPO_B_SUBMODULE_PATH).as_str())
        .add_file(
            format!("{}/B_B", REPO_B_SUBMODULE_PATH).as_str(),
            "Changing B_B in static copy of repo_b",
        )
        .add_file(
            format!("{}/B_C", REPO_B_SUBMODULE_PATH).as_str(),
            "Add file to static copy of repo_b",
        )
        .commit()
        .await?;

    let (large_repo_cs_id, large_repo_changesets) = sync_changeset_and_derive_all_types(
        ctx.clone(),
        chg_sm_path_cs_id,
        &large_repo,
        &commit_syncer,
    )
    .await?;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[
            // Changeset that deletes the submodule metadata file
            ExpectedChangeset::new(DELETE_METADATA_FILE_MSG)
                .with_regular_changes(
                    // The submodule files are treated as regular file changes
                    vec![
                        "small_repo/submodules/repo_b/B_A",
                        "small_repo/submodules/repo_b/B_B",
                    ],
                )
                .with_deletions(
                    // Only submodule metadata file is deleted
                    vec!["small_repo/submodules/.x-repo-submodule-repo_b"],
                ),
            // Changeset that modifies files in the submodule path, which is
            // now a static copy of the submodule
            ExpectedChangeset::new(CHANGE_SUBMODULE_PATH_MSG)
                .with_regular_changes(
                    // The submodule files are treated as regular file changes
                    vec![
                        "small_repo/submodules/repo_b/B_B",
                        "small_repo/submodules/repo_b/B_C",
                    ],
                )
                .with_deletions(
                    // Only submodule metadata file is deleted
                    vec!["small_repo/submodules/repo_b/B_A"],
                ),
        ],
    )?;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
            // Files from the submodule are now regular files in the small repo
            "small_repo/submodules/repo_b/B_B",
            "small_repo/submodules/repo_b/B_C",
        ],
    )
    .await?;

    // Check mappings of both commits
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        del_md_file_cs_id,
        Some(first_expected_cs_id),
    )
    .await;
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        chg_sm_path_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    Ok(())
}

/// Same scenario as `test_deleting_submodule_but_keeping_directory`, but with
/// a recursive submodule.
#[mononoke::fbinit_test]
async fn test_deleting_recursive_submodule_but_keeping_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_git_sha1 = git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"]).await?;

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            (repo_c_submodule_path.clone(), repo_c.clone()),
        ],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    const DELETE_METADATA_FILE_MSG: &str = "Delete repo_c submodule and keep its static copy";

    let del_repo_c_md_file_cs_id =
        CreateCommitContext::new(&ctx, &repo_b, vec![repo_b_cs_map["B_B"]])
            .set_message(DELETE_METADATA_FILE_MSG)
            .delete_file(repo_c_submodule_path_in_repo_b.clone())
            // Delete the submodule file change, but keep the contents in the same path
            .add_file(
                repo_c_submodule_path_in_repo_b
                    .clone()
                    .join(&NonRootMPath::new("C_A")?),
                "first commit in submodule C",
            )
            .add_file(
                repo_c_submodule_path_in_repo_b
                    .clone()
                    .join(&NonRootMPath::new("C_B")?),
                "second commit in submodule C",
            )
            .commit()
            .await?;

    let repo_b_git_commit_hash =
        git_sha1_from_changeset(&ctx, &repo_b, del_repo_c_md_file_cs_id).await?;

    let del_md_file_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(DELETE_METADATA_FILE_MSG)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                repo_b_git_commit_hash.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, del_md_file_cs_id)
        .await
        .context("Failed to sync del_md_file_cs_id")
        .and_then(|res| res.ok_or(anyhow!("No commit was synced")))?;

    let first_expected_cs_id = large_repo_cs_id;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
            // Files from the submodule are now regular files in the small repo
            "small_repo/submodules/.x-repo-submodule-repo_b",
            "small_repo/submodules/repo_b/B_A",
            "small_repo/submodules/repo_b/B_B",
            "small_repo/submodules/repo_b/submodules/repo_c/C_A",
            "small_repo/submodules/repo_b/submodules/repo_c/C_B",
        ],
    )
    .await?;

    const CHANGE_SUBMODULE_PATH_MSG: &str = "Change static copy of repo_c";

    let chg_repo_c_copy_cs_id =
        CreateCommitContext::new(&ctx, &repo_b, vec![del_repo_c_md_file_cs_id])
            .set_message(CHANGE_SUBMODULE_PATH_MSG)
            // Modify files in the submodule path, because they're now regular files
            // in the small repo
            .delete_file(
                repo_c_submodule_path_in_repo_b
                    .clone()
                    .join(&NonRootMPath::new("C_A")?),
            )
            .add_file(
                repo_c_submodule_path_in_repo_b
                    .clone()
                    .join(&NonRootMPath::new("C_B")?),
                "Change file in static copy of repo_c",
            )
            .add_file(
                repo_c_submodule_path_in_repo_b.join(&NonRootMPath::new("C_C")?),
                "Add file to static copy of repo_c",
            )
            .commit()
            .await?;

    let repo_b_git_commit_hash =
        git_sha1_from_changeset(&ctx, &repo_b, chg_repo_c_copy_cs_id).await?;

    let chg_sm_path_cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![del_md_file_cs_id])
        .set_message(CHANGE_SUBMODULE_PATH_MSG)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let (large_repo_cs_id, large_repo_changesets) = sync_changeset_and_derive_all_types(
        ctx.clone(),
        chg_sm_path_cs_id,
        &large_repo,
        &commit_syncer,
    )
    .await?;

    compare_expected_changesets(
        large_repo_changesets.last_chunk::<2>().unwrap(),
        &[
            // Changeset that deletes the submodule metadata file
            ExpectedChangeset::new(DELETE_METADATA_FILE_MSG)
                .with_regular_changes(
                    // The submodule files are treated as regular file changes
                    vec![
                        // repo_b submodule metadata file is updated
                        "small_repo/submodules/.x-repo-submodule-repo_b",
                        "small_repo/submodules/repo_b/submodules/repo_c/C_A",
                        "small_repo/submodules/repo_b/submodules/repo_c/C_B",
                    ],
                )
                .with_deletions(
                    // Only submodule metadata file is deleted
                    vec!["small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c"],
                ),
            // Changeset that modifies files in the submodule path, which is
            // now a static copy of the submodule
            ExpectedChangeset::new(CHANGE_SUBMODULE_PATH_MSG)
                .with_regular_changes(
                    // The submodule files are treated as regular file changes
                    vec![
                        // repo_b submodule metadata file is updated
                        "small_repo/submodules/.x-repo-submodule-repo_b",
                        "small_repo/submodules/repo_b/submodules/repo_c/C_B",
                        "small_repo/submodules/repo_b/submodules/repo_c/C_C",
                    ],
                )
                .with_deletions(
                    // Only submodule metadata file is deleted
                    vec!["small_repo/submodules/repo_b/submodules/repo_c/C_A"],
                ),
        ],
    )?;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
            // Files from the submodule are now regular files in the small repo
            "small_repo/submodules/.x-repo-submodule-repo_b",
            "small_repo/submodules/repo_b/B_A",
            "small_repo/submodules/repo_b/B_B",
            "small_repo/submodules/repo_b/submodules/repo_c/C_B",
            "small_repo/submodules/repo_b/submodules/repo_c/C_C",
        ],
    )
    .await?;

    // Check mappings of both commits
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        del_md_file_cs_id,
        Some(first_expected_cs_id),
    )
    .await;
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        chg_sm_path_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    Ok(())
}

// ------------------------- Implicit deletes ----------------------------

// Cover implicit deletions.
// Implicitly deleting **a submodule** means adding a file in **repo A** at the
// same path of the repo B submodule.
// This should delete repo B's expansion in the large repo along with its
// metadata file.
//
// This differs from implicitly deleting directories **in the submodule repo**,
// which corresponds to an implicit deletion **in repo B** that should be propagated
// to its expansion in the large repo.

/// Implicitly deleting a submodule in the source repo (i.e. by adding a file
/// with the same path) should delete the **entire submodule expansion and its
/// metadata file**, like when the submodule itself is manually deleted.
#[mononoke::fbinit_test]
async fn test_implicitly_deleting_submodule(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    const MESSAGE: &str = "Implicitly delete repo_b submodule in small_repo";

    let cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
        .set_message(MESSAGE)
        .add_file(REPO_B_SUBMODULE_PATH, "File implicitly deleting submodule")
        .commit()
        .await?;

    let (large_repo_cs_id, large_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &large_repo, &commit_syncer)
            .await?;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[ExpectedChangeset::new(MESSAGE)
            .with_regular_changes(
                // Add a regular file in the same path as the submodule expansion
                vec!["small_repo/submodules/repo_b"],
            )
            .with_deletions(
                // Files being deleted
                vec![
                    // The submodule metadata file should also be deleted
                    "small_repo/submodules/.x-repo-submodule-repo_b",
                    // NOTE: no need to have explicit deletions for these files, because
                    // they're being deleted implicitly.
                    // "small_repo/submodules/repo_b/B_A",
                    // "small_repo/submodules/repo_b/B_B",
                ],
            )],
    )?;

    // Assert that the submodule expansion was actually deleted implicitly
    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
            "small_repo/submodules/repo_b",
        ],
    )
    .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(large_repo_cs_id)).await;
    Ok(())
}

/// Implicitly deleting files in the submodule repo (repo_b) should generate the
/// proper deletions in its expansion.
#[mononoke::fbinit_test]
async fn test_implicit_deletions_inside_submodule_repo(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    // Create a directory with 2 files in repo B
    let add_directory_in_repo_b =
        CreateCommitContext::new(&ctx, &repo_b, vec![*repo_b_cs_map.get("B_B").unwrap()])
            .set_message("Create directory in repo B")
            .add_files(btreemap! {"some_dir/file_x" => "File X", "some_dir/file_y" => "File Y"})
            .commit()
            .await?;

    // Create a file in the same path to implicitly delete the directory
    let repo_b_cs_id = CreateCommitContext::new(&ctx, &repo_b, vec![add_directory_in_repo_b])
        .set_message("Implicitly delete directory in repo B")
        .add_file("some_dir", "some_dir is now a file")
        .commit()
        .await?;

    let repo_b_git_commit_hash = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id).await?;

    // Update repo B submodule pointer in repo A to point to the last commit
    // with the implicit deletions.
    const MESSAGE: &str = "Update submodule after implicit deletions";
    let cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
        .set_message(MESSAGE)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let (large_repo_cs_id, large_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &large_repo, &commit_syncer)
            .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(large_repo_cs_id)).await;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[
            ExpectedChangeset::new(MESSAGE).with_regular_changes(
                // Submodule metadata file is updated
                vec![
                    "small_repo/submodules/.x-repo-submodule-repo_b",
                    "small_repo/submodules/repo_b/some_dir",
                ],
            ),
            // NOTE: no need to have explicit deletions for these files, because
            // they're being deleted implicitly:
            // "small_repo/submodules/repo_b/some_dir/file_x",
            // "small_repo/submodules/repo_b/some_dir/file_y"
        ],
    )?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    // Assert that `file_x` and `file_y` are not in the working copy
    // by getting all leaves from the RootFsnode
    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "small_repo/A_A",
            "small_repo/A_B",
            "small_repo/A_C",
            "small_repo/submodules/.x-repo-submodule-repo_b",
            "small_repo/submodules/repo_b/B_A",
            "small_repo/submodules/repo_b/B_B",
            "small_repo/submodules/repo_b/some_dir",
        ],
    )
    .await?;

    Ok(())
}

/// Test adding a submodule dependency in the source repo in the path of an existing
/// file. This should generate a deletion of the file in the large repo, along
/// with the expansion of the submodule.
#[mononoke::fbinit_test]
async fn test_implicitly_deleting_file_with_submodule(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    // Create repo C, to be added as a submodule in repo A.
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        live_commit_sync_config,
        test_sync_config_source,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        // Initial config should only have repo B as submodule dependency,
        // because the test data setup will create a file in the path `A_A`
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    // Update the commit syncer to use a new config version with extra submodule
    // dependencies.
    // This config version will include the submodule that will be added in the
    // submodule deps.
    let commit_syncer = add_new_commit_sync_config_version_with_submodule_deps(
        &ctx,
        &small_repo,
        &large_repo,
        "small_repo",
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            // Add it as a submdule in the path of the existing `A_A` file.
            (NonRootMPath::new("A_A").unwrap(), repo_c.clone()),
        ],
        live_commit_sync_config,
        test_sync_config_source,
        vec![], // Known dangling submodule pointers
    )?;

    let repo_c_git_commit_hash =
        git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"]).await?;

    const MESSAGE: &str = "Add submodule on path of existing file";
    let cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
        .set_message(MESSAGE)
        .add_file_with_type(
            "A_A",
            repo_c_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let (large_repo_cs_id, large_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &large_repo, &commit_syncer)
            .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(large_repo_cs_id)).await;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[ExpectedChangeset::new(MESSAGE)
            .with_regular_changes(vec![
                "small_repo/.x-repo-submodule-A_A",
                "small_repo/A_A/C_A",
                "small_repo/A_A/C_B",
            ])
            .with_deletions(vec![
                // The original file is deleted because of the submodule expansion
                "small_repo/A_A",
            ])],
    )?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/.x-repo-submodule-A_A")?,
        &repo_c_git_commit_hash,
    )
    .await?;

    Ok(())
}

/// Test adding a submodule dependency in the source repo in the path of an
/// existing **directory**. This should generate a deletion for all the files
/// in the directory, along with the expansion of the submodule.
#[mononoke::fbinit_test]
async fn test_adding_submodule_on_existing_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    // Create repo C, to be added as a submodule in repo A.
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let dir_path = NonRootMPath::new("some_dir/subdir")?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        live_commit_sync_config,
        test_sync_config_source,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        // Add it as a submdule in the path of an existing directory.
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    const ADD_DIR_MSG: &str = "Create directory with a few files";
    let add_dir_cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
        .set_message(ADD_DIR_MSG)
        .add_files(btreemap! {
            dir_path.join(&NonRootMPath::new("file_x")?) => "File X",
            dir_path.join(&NonRootMPath::new("file_y")?) => "File Y",
            dir_path.join(&NonRootMPath::new("file_z")?) => "File Z",
            // Adding a file that exists in the submodule and will be mapped
            // to the same path here, so ensure that we deduplicate properly
            dir_path.join(&NonRootMPath::new("C_A")?) => "Same path as file in submodule",
        })
        .commit()
        .await?;

    let _ = sync_to_master(ctx.clone(), &commit_syncer, add_dir_cs_id)
        .await
        .context("Failed to sync commit creating normal directory with files")?;

    // Update the commit syncer to use a new config version with extra submodule
    // dependencies.
    // This config version will include the submodule that will be added in the
    // path of an existing directory.
    let commit_syncer = add_new_commit_sync_config_version_with_submodule_deps(
        &ctx,
        &small_repo,
        &large_repo,
        "small_repo",
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            // Add the submodule path to the config
            (dir_path.clone(), repo_c.clone()),
        ],
        live_commit_sync_config,
        test_sync_config_source,
        vec![], // Known dangling submodule pointers
    )?;

    let repo_c_git_commit_hash =
        git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"]).await?;

    const MESSAGE: &str = "Add submodule on path of existing directory";
    let cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![add_dir_cs_id])
        .set_message(MESSAGE)
        .add_file_with_type(
            dir_path,
            repo_c_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let (large_repo_cs_id, large_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &large_repo, &commit_syncer)
            .await?;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[
            ExpectedChangeset::new(ADD_DIR_MSG).with_regular_changes(vec![
                "small_repo/some_dir/subdir/file_x",
                "small_repo/some_dir/subdir/file_y",
                "small_repo/some_dir/subdir/file_z",
                "small_repo/some_dir/subdir/C_A",
            ]),
            ExpectedChangeset::new(MESSAGE)
                .with_regular_changes(vec![
                    "small_repo/some_dir/.x-repo-submodule-subdir",
                    "small_repo/some_dir/subdir/C_A",
                    "small_repo/some_dir/subdir/C_B",
                ])
                .with_deletions(vec![
                    // All files from the directory should be deleted with
                    // the addition of a submodule expansion on the same path
                    "small_repo/some_dir/subdir/file_x",
                    "small_repo/some_dir/subdir/file_y",
                    "small_repo/some_dir/subdir/file_z",
                    // NOTE: We DON'T actually want a deletion for C_A, because
                    // the submodule expansion has the file with the same path.
                    // I'm leaving this commented out to convey this clearly.
                    // "small_repo/some_dir/subdir/C_A",
                ]),
        ],
    )?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/some_dir/.x-repo-submodule-subdir")?,
        &repo_c_git_commit_hash,
    )
    .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(large_repo_cs_id)).await;

    Ok(())
}

// ------------------ Unexpected state / Error handling ------------------

/// Test that sync fails if submodule dependency repo is not available.
#[mononoke::fbinit_test]
async fn test_submodule_expansion_crashes_when_dep_not_available(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    // Create repo C, to be added as a submodule in repo A.
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        // Don't pass repo C as a submodule dependency of repo A
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    // Get a git commit from repo C
    let repo_c_git_commit_hash =
        git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"]).await?;

    // Create a commit in repo A that adds repo C as a submodule.
    const MESSAGE: &str = "Add submodule on path of existing file";
    let cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
        .set_message(MESSAGE)
        .add_file_with_type(
            "submodules/repo_c",
            repo_c_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await;

    println!("sync_result: {0:#?}", &sync_result);

    assert!(sync_result.is_err_and(|err| {
        err.chain().any(|e| {
            // Make sure that we're throwing because the submodule repo is not available
            e.to_string()
                .contains("Mononoke repo from submodule submodules/repo_c not available")
        })
    }));

    // Get all the changesets in the large repo
    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;

    derive_all_enabled_types_for_repo(&ctx, &large_repo, large_repo_changesets.as_slice()).await?;

    // And confirm that nothing was synced, i.e. all changesets are from the basic
    // setup.
    compare_expected_changesets_from_basic_setup(&large_repo_changesets, &[])?;

    Ok(())
}

/// Tests that validation fails if a user adds a file in the original repo
/// matching the path of a submodule metadata file.
///
/// It's an unlikely scenario, but we want to be certain of what would happen,
/// because users might, for example, manually copy directories from the large
/// repo to the git repo.
#[mononoke::fbinit_test]
async fn test_submodule_validation_fails_with_file_on_metadata_file_path_in_small_repo(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    const MESSAGE_CS_1: &str =
        "Add file with same path as a submodule metadata file with random content";

    let small_repo_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(MESSAGE_CS_1)
            .add_file(
                "submodules/.x-repo-submodule-repo_b",
                "File that should only exist in the large repo",
            )
            .commit()
            .await?;

    println!("Trying to sync changeset #1!");

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, small_repo_cs_id)
        .await
        .context("sync_to_master failed")
        .and_then(|res| res.ok_or(anyhow!("No commit was synced")));

    println!("sync_result from changeset #1: {0:#?}", &sync_result);

    // TODO(T174902563): fail EXPANSION because of path overlap
    // Currently we're failing VALIDATION because the content is not a valid
    // git hash, but ideally we want submodule EXPANSION to fail.
    // let expected_err_msg =
    //     "User file changes clash paths with generated changes for submodule expansion";
    // assert!(sync_result.is_err_and(|err| {
    //     err.chain()
    //         .any(|e| e.to_string().contains(expected_err_msg))
    // }));

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;
    println!("large_repo_changesets: {:#?}\n\n", &large_repo_changesets);

    // When this is fixed, the commit sync should fail, instead of validation.
    // check_mapping(ctx.clone(), &commit_syncer, small_repo_cs_id, None).await;

    // Do the same thing, but adding a valid git commit has in the file
    // To see what happens if a user tries updating a submodule in a weird
    // unexpected way.
    const MESSAGE_CS_2: &str =
        "Add file with same path as a submodule metadata file with valid git commit hash";

    let repo_b_git_commit_hash =
        git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_map["B_A"]).await?;

    let small_repo_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(MESSAGE_CS_2)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                repo_b_git_commit_hash.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    println!("Trying to sync changeset #2!");
    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, small_repo_cs_id)
        .await
        .context("sync_to_master failed")
        .and_then(|res| res.ok_or(anyhow!("No commit was synced")));

    println!("sync_result from changeset #2: {0:#?}", &sync_result);

    // TODO(T174902563): fail expansion because of path overlap
    // let expected_err_msg =
    //     "User file changes clash paths with generated changes for submodule expansion";
    // assert!(sync_result.is_err_and(|err| {
    //     err.chain()
    //         .any(|e| e.to_string().contains(expected_err_msg))
    // }));

    // When this is fixed, the commit sync should fail, instead of validation.
    // check_mapping(ctx.clone(), &commit_syncer, small_repo_cs_id, None).await;

    Ok(())
}

/// Similar to the test above, but adding a file that maps to a submodule
/// metadata file path of a recursive submodule.
#[mononoke::fbinit_test]
async fn test_submodule_validation_fails_with_file_on_metadata_file_path_in_recursive_submodule(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let c_master_git_sha1 = git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"]).await?;

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);
    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (_large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            (repo_c_submodule_path, repo_c.clone()),
        ],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    // Modify repo_b, adding a file that when synced to the large repo will have
    // the same path as the submodule metadata file for repo_c submodule.
    let repo_b_cs_id = CreateCommitContext::new(&ctx, &repo_b, vec![repo_b_cs_map["B_B"]])
        .set_message("Add file with same path as a submodule metadata file")
        .add_file(
            "submodules/.x-repo-submodule-repo_c",
            "File that should only exist in the large repo",
        )
        .commit()
        .await?;

    let repo_b_git_commit_hash = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id).await?;

    const MESSAGE: &str =
        "Update repo_b submodule after adding a file in the same place as a metadata file";

    let small_repo_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(MESSAGE)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                repo_b_git_commit_hash.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, small_repo_cs_id)
        .await
        .context("sync_to_master failed")
        .and_then(|res| res.ok_or(anyhow!("No commit was synced")));

    println!("sync_result: {0:#?}", &sync_result);

    // TODO(T174902563): fail EXPANSION because of path overlap
    // Currently we're failing VALIDATION because the content is not a valid
    // git hash, but ideally we want submodule EXPANSION to fail.
    // let expected_err_msg =
    //     "User file changes clash paths with generated changes for submodule expansion";
    // assert!(sync_result.is_err_and(|err| {
    //     err.chain()
    //         .any(|e| e.to_string().contains(expected_err_msg))
    // }));

    // When this is fixed, the commit sync should fail, instead of validation.
    // check_mapping(ctx.clone(), &commit_syncer, small_repo_cs_id, None).await;

    Ok(())
}

/// Test that expanding a **known** dangling submodule pointer works as expected.
/// A commit is created in the submodule repo with a README file informing that
/// the pointer didn't exist in the submodule.
/// The list of known dangling submodule pointers can be set in the small repo's
/// sync config.
#[mononoke::fbinit_test]
async fn test_expanding_known_dangling_submodule_pointers(fb: FacebookInit) -> Result<()> {
    pub const REPO_B_DANGLING_GIT_COMMIT_HASH: &str = "e957dda44445098cfbaea99e4771e737944e3da4";
    pub const REPO_C_DANGLING_GIT_COMMIT_HASH: &str = "408dc1a8d40f13a0b8eee162411dba2b8830b1f0";

    let known_dangling_submodule_pointers = vec![
        REPO_B_DANGLING_GIT_COMMIT_HASH,
        REPO_C_DANGLING_GIT_COMMIT_HASH,
    ];
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_git_sha1 = git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"]).await?;

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;
    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            (repo_c_submodule_path, repo_c.clone()),
        ],
        known_dangling_submodule_pointers,
    )
    .await?;

    // COMMIT 1: set a dangling pointer in repo_b submodule
    const COMMIT_MSG_1: &str = "Set submodule pointer to known dangling pointer from config";

    // Test expanding a known dangling submodule pointer
    let repo_b_dangling_pointer = GitSha1::from_str(REPO_B_DANGLING_GIT_COMMIT_HASH)?;

    let small_repo_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(COMMIT_MSG_1)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                repo_b_dangling_pointer.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, small_repo_cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    // Look for the README file and assert its content matches expectation
    let bonsai = large_repo_cs_id
        .load(&ctx, large_repo.repo_blobstore())
        .await
        .context("Failed to load bonsai in large repo")?;
    let readme_file_change = bonsai
        .file_changes_map()
        .get(&NonRootMPath::new(
            "small_repo/submodules/repo_b/README.TXT",
        )?)
        .ok_or(anyhow!(
            "No file change for README file about dangling submodule pointer"
        ))?;
    let readme_file_content_id = readme_file_change
        .simplify()
        .expect("Should be a file change with content id")
        .content_id();
    let readme_file_bytes =
        filestore::fetch_concat(large_repo.repo_blobstore(), &ctx, readme_file_content_id).await?;
    let readme_file_content = std::str::from_utf8(readme_file_bytes.as_ref())?;

    assert_eq!(
        readme_file_content,
        "This is the expansion of a known dangling submodule pointer e957dda44445098cfbaea99e4771e737944e3da4. This commit doesn't exist in the repo repo_b",
        "Dangling submodule pointer expansion README file doesn't have the right content"
    );

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_dangling_pointer,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    // COMMIT 2: set repo_b submodule pointer to a new valid commit

    const COMMIT_MSG_2: &str = "Fix repo_b submodule pointer";

    let repo_b_cs_id =
        CreateCommitContext::new(&ctx, &repo_b, vec![*repo_b_cs_map.get("B_B").unwrap()])
            .set_message("Add file to repo_b")
            .add_file("B_C", "new file content")
            .commit()
            .await?;

    let repo_b_git_commit_hash = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id).await?;

    let small_repo_cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_id])
        .set_message(COMMIT_MSG_2)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, small_repo_cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    // COMMIT 3: set a dangling pointer in repo_c submodule in repo_b to test
    // dangling pointers in recursive submodules.

    const COMMIT_MSG_3: &str = "Set repo_c recursive submodule pointer to known dangling pointer";

    let repo_c_dangling_pointer = GitSha1::from_str(REPO_C_DANGLING_GIT_COMMIT_HASH)?;

    let repo_b_cs_id = CreateCommitContext::new(&ctx, &repo_b, vec![repo_b_cs_id])
        .set_message("Set dangling pointer in repo_c recursive submodule")
        .add_file_with_type(
            repo_c_submodule_path_in_repo_b.clone(),
            repo_c_dangling_pointer.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let repo_b_git_commit_hash = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id).await?;

    let small_repo_cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_id])
        .set_message(COMMIT_MSG_3)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, small_repo_cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    // COMMIT 4: set repo_c recursive submodule pointer to valid commit
    const COMMIT_MSG_4: &str = "Fix repo_c recursive submodule by pointing it back to C_B";

    let repo_b_cs_id = CreateCommitContext::new(&ctx, &repo_b, vec![repo_b_cs_id])
        .set_message("Fix dangling pointer in repo_c recursive submodule")
        .add_file_with_type(
            repo_c_submodule_path_in_repo_b,
            c_master_git_sha1.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let repo_b_git_commit_hash = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id).await?;

    let small_repo_cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_id])
        .set_message(COMMIT_MSG_4)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, small_repo_cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c")?,
        &c_master_git_sha1,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    // ------------------ Assertions / validations ------------------

    // Get all the changesets
    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;

    // Ensure everything can be derived successfully
    derive_all_enabled_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

    compare_expected_changesets(
        large_repo_changesets.last_chunk::<4>().unwrap(),
        &[
            // COMMIT 1: Expansion of known dangling pointer
            ExpectedChangeset::new(COMMIT_MSG_1)
                .with_regular_changes(vec![
                    // Submodule metadata file is updated
                    "small_repo/submodules/.x-repo-submodule-repo_b",
                    // README file is added with a message informing that this
                    // submodule pointer was dangling.
                    "small_repo/submodules/repo_b/README.TXT",
                ])
                .with_deletions(
                    // Should delete everything from previous expansion
                    vec![
                        "small_repo/submodules/repo_b/B_A",
                        "small_repo/submodules/repo_b/B_B",
                        "small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                        "small_repo/submodules/repo_b/submodules/repo_c/C_A",
                        "small_repo/submodules/repo_b/submodules/repo_c/C_B",
                    ],
                ),
            // COMMIT 2: Fix the dangling pointer
            ExpectedChangeset::new(COMMIT_MSG_2)
                .with_regular_changes(vec![
                    // Submodule metadata file is updated
                    "small_repo/submodules/.x-repo-submodule-repo_b",
                    // Add back files from previous submodule pointer
                    "small_repo/submodules/repo_b/B_A",
                    "small_repo/submodules/repo_b/B_B",
                    "small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                    "small_repo/submodules/repo_b/submodules/repo_c/C_A",
                    "small_repo/submodules/repo_b/submodules/repo_c/C_B",
                    // Plus the new file added in the new valid pointer
                    "small_repo/submodules/repo_b/B_C",
                ])
                .with_deletions(vec![
                    // Delete README file from dangling pointer expansion
                    "small_repo/submodules/repo_b/README.TXT",
                ]),
            // COMMIT 3: Set dangling pointer in repo_c recursive submodule
            ExpectedChangeset::new(COMMIT_MSG_3)
                .with_regular_changes(vec![
                    // Submodule metadata files are updated
                    "small_repo/submodules/.x-repo-submodule-repo_b",
                    "small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                    // README file is added with a message informing that this
                    // submodule pointer was dangling.
                    "small_repo/submodules/repo_b/submodules/repo_c/README.TXT",
                ])
                .with_deletions(
                    // Should delete everything from previous expansion
                    vec![
                        "small_repo/submodules/repo_b/submodules/repo_c/C_A",
                        "small_repo/submodules/repo_b/submodules/repo_c/C_B",
                    ],
                ),
            // COMMIT 4: Fix dangling pointer in repo_c recursive submodule
            ExpectedChangeset::new(COMMIT_MSG_4)
                .with_regular_changes(vec![
                    // Submodule metadata files are updated
                    "small_repo/submodules/.x-repo-submodule-repo_b",
                    "small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                    // Add back files from expansion of commit C_B
                    "small_repo/submodules/repo_b/submodules/repo_c/C_A",
                    "small_repo/submodules/repo_b/submodules/repo_c/C_B",
                ])
                .with_deletions(vec![
                    // Delete README file from dangling pointer expansion
                    "small_repo/submodules/repo_b/submodules/repo_c/README.TXT",
                ]),
        ],
    )?;

    Ok(())
}

/// Test that submodule expansion updates and deletions will work for merge
/// commits.
#[mononoke::fbinit_test]
async fn test_submodule_expansion_and_deletion_on_merge_commits(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        vec![], // Known dangling submodule pointers
    )
    .await?;

    // Standalone commit with no parent, because we don't support syncing diamond merges
    // through forward syncer.
    let p1_cs_id = CreateCommitContext::new(&ctx, &small_repo, Vec::<ChangesetId>::new())
        .set_message("Parent commit P1")
        .add_file("file_from_p1", "file_from_p1")
        .commit()
        .await?;

    // Another commit to be used as a parent
    let p2_cs_id = CreateCommitContext::new(&ctx, &small_repo, Vec::<ChangesetId>::new())
        .set_message("Parent commit P2")
        .add_file("file_from_p2", "file_from_p2")
        .commit()
        .await?;

    // Sync both standalone commits, because we can't sync any commits without
    // first syncing all of their parents.
    let _large_p1_cs_id = commit_syncer
        .unsafe_sync_commit(
            &ctx,
            p1_cs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::XRepoSyncJob,
            Some(base_commit_sync_version_name()),
            false, // add_mapping_to_hg_extra
        )
        .await
        .context("Failed to sync standalone parent commit")?;

    let _large_p2_cs_id = commit_syncer
        .unsafe_sync_commit(
            &ctx,
            p2_cs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::XRepoSyncJob,
            Some(base_commit_sync_version_name()),
            false, // add_mapping_to_hg_extra
        )
        .await
        .context("Failed to sync standalone parent commit")?;

    // COMMIT 1: MERGE commit updating the repo_b submodule pointer
    let repo_b_git_commit_hash =
        git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_map["B_A"]).await?;

    const MESSAGE_1: &str = "Change repo_b submodule with two parent commits";
    let cs_id_1 =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"], p1_cs_id])
            .set_message(MESSAGE_1)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                repo_b_git_commit_hash.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let large_repo_cs_id_1 = sync_to_master(ctx.clone(), &commit_syncer, cs_id_1)
        .await
        .context("Failed to sync del_md_file_cs_id")
        .and_then(|res| res.ok_or(anyhow!("No commit was synced")))?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        cs_id_1,
        Some(large_repo_cs_id_1),
    )
    .await;

    // COMMIT 2: MERGE commit deleting the repo_b submodule
    const MESSAGE_2: &str = "Delete repo_b submodule with two parent commits";
    let cs_id_2 = CreateCommitContext::new(&ctx, &small_repo, vec![cs_id_1, p2_cs_id])
        .set_message(MESSAGE_2)
        .delete_file(REPO_B_SUBMODULE_PATH)
        .commit()
        .await?;

    let (large_repo_cs_id_2, large_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id_2, &large_repo, &commit_syncer)
            .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        cs_id_2,
        Some(large_repo_cs_id_2),
    )
    .await;

    compare_expected_changesets(
        large_repo_changesets.last_chunk::<2>().unwrap(),
        &[
            ExpectedChangeset::new(MESSAGE_1)
                .with_regular_changes(vec!["small_repo/submodules/.x-repo-submodule-repo_b"])
                .with_deletions(vec!["small_repo/submodules/repo_b/B_B"]),
            ExpectedChangeset::new(MESSAGE_2).with_deletions(vec![
                "small_repo/submodules/.x-repo-submodule-repo_b",
                "small_repo/submodules/repo_b/B_A",
            ]),
        ],
    )?;

    Ok(())
}

/// Test what happens if we accidentally put commits that exist as dangling pointers.
/// This can lead to a limbo state, where further commits with valid submodule
/// pointers fail to expand.
/// This is a known issue and this test is just to document the behavior.
#[mononoke::fbinit_test]
async fn test_expanding_existing_submdule_commits_as_dangling_pointers(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await?;

    // These are the git hashes from the repo_b commits that will be created after
    // in the tests. They were obtained by running the tests and creating the commits
    // before, to hardcode them.
    let B_C_git_sha1 = GitSha1::from_str("f614297935f4abe6132f30f8c2dad26d9a4f5fde")?;
    let B_D_git_sha1 = GitSha1::from_str("256cf085be052fd8126de1fca2b28c859c56b28d")?;

    // Create repo_a treating the 2 commits that will be created in repo_b as
    // dangling pointers.
    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
        // Known dangling submodule pointers
        vec![&B_C_git_sha1.to_string(), &B_D_git_sha1.to_string()],
    )
    .await?;

    const MESSAGE_1: &str = "Expand FIRST dangling pointers that exists";

    let small_repo_cs_id_1 =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_map["A_C"]])
            .set_message(MESSAGE_1)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                B_C_git_sha1.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let (large_repo_cs_id, large_repo_changesets) = sync_changeset_and_derive_all_types(
        ctx.clone(),
        small_repo_cs_id_1,
        &large_repo,
        &commit_syncer,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        small_repo_cs_id_1,
        Some(large_repo_cs_id),
    )
    .await;

    // Since B_C didn't exist, it will be rightfully expanded as a dangling pointer.
    compare_expected_changesets(
        large_repo_changesets.last_chunk::<1>().unwrap(),
        &[ExpectedChangeset::new(MESSAGE_1)
            .with_regular_changes(vec![
                // Submodule metadata file is updated
                "small_repo/submodules/.x-repo-submodule-repo_b",
                // README file is added with a message informing that this
                // submodule pointer was dangling.
                "small_repo/submodules/repo_b/README.TXT",
            ])
            .with_deletions(
                // Should delete everything from previous expansion
                vec![
                    "small_repo/submodules/repo_b/B_A",
                    "small_repo/submodules/repo_b/B_B",
                ],
            )],
    )?;

    // STEP 2: Now create commits B_C and B_D in repo_b. This is equivalent to
    // making a mistake in the sync config, treating a submodule repo as another,
    // then updating the config and now the commits exist.
    let B_C_cs_id = CreateCommitContext::new(&ctx, &repo_b, vec![repo_b_cs_map["B_B"]])
        .set_message("FIRST commit that exists but will be expanded as dangling")
        .add_file("repo_b_file", "new file content")
        .commit()
        .await?;

    let B_D_cs_id = CreateCommitContext::new(&ctx, &repo_b, vec![B_C_cs_id])
        .set_message("SECOND commit that exists but will be expanded as dangling")
        .add_file("repo_b_file", "change file")
        .commit()
        .await?;

    let B_C_real_git_sha1 = git_sha1_from_changeset(&ctx, &repo_b, B_C_cs_id).await?;
    let B_D_real_git_sha1 = git_sha1_from_changeset(&ctx, &repo_b, B_D_cs_id).await?;

    println!("B_C_real_git_sha1: {B_C_real_git_sha1}");
    println!("B_D_real_git_sha1: {B_D_real_git_sha1}");

    assert_eq!(
        B_C_real_git_sha1.to_string(),
        B_C_git_sha1.to_string(),
        "B_C git hashes don't match. Update hardcoded value!"
    );
    assert_eq!(
        B_D_real_git_sha1.to_string(),
        B_D_git_sha1.to_string(),
        "B_D git hashes don't match. Update hardcoded value!"
    );

    // STEP 3: Now, try to expand commit B_D as danling, even though it exists
    // and its parent, B_C, also exists but was expanded as dangling.
    const MESSAGE_2: &str = "Expand SECOND dangling pointers that exists";
    let small_repo_cs_id_2 = CreateCommitContext::new(&ctx, &small_repo, vec![small_repo_cs_id_1])
        .set_message(MESSAGE_2)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            B_D_git_sha1.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, small_repo_cs_id_2).await;

    println!("sync_result: {0:#?}", &sync_result);

    assert!(sync_result.is_err_and(|err| {
        err.chain().any(|e| {
            // Make sure that we're throwing because the submodule repo is not available
            e.to_string()
                .contains("Path B_A is in submodule manifest but not in expansion")
                || e.to_string()
                    .contains("Path B_B is in submodule manifest but not in expansion")
        })
    }));

    Ok(())
}
