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
use maplit::btreemap;
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
    } = build_submodule_sync_test_data(fb, vec![]).await?;

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

    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

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
        &large_repo_changesets,
        &[ExpectedChangeset::new_by_file_change(
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

// ------------------------- Deletions ----------------------------

/// Deleting an entire submodule in the small repo (i.e. repo_a) should delete
/// its expansion and its metadata file in repo_a folder in the large repo.
#[fbinit::test]
async fn test_submodule_deletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(fb, vec![]).await?;

    const MESSAGE: &str = "Delete repo_b submodule in repo_a";
    let cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![*repo_a_cs_map.get("A_C").unwrap()])
        .set_message(MESSAGE)
        .delete_file("submodules/repo_b")
        .commit()
        .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit deleting submodule B"))?;

    println!("large_repo_cs_id: {}", large_repo_cs_id);

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;

    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        cs_id,
        ChangesetId::from_str("04fc8faa78bf6eb8e3f75fa34ba823577c8b78cb4428e881b0d7b7db956630b5")
            .ok(),
    )
    .await;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[ExpectedChangeset::new_by_file_change(
            MESSAGE,
            // No regular file changes
            vec![],
            // Files being deleted
            vec![
                "repo_a/submodules/.x-repo-submodule-repo_b",
                "repo_a/submodules/repo_b/B_A",
                "repo_a/submodules/repo_b/B_B",
            ],
        )],
    )?;

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
#[fbinit::test]
async fn test_implicitly_deleting_submodule(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(fb, vec![]).await?;

    const MESSAGE: &str = "Implicitly delete repo_b submodule in repo_a";

    let cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![*repo_a_cs_map.get("A_C").unwrap()])
        .set_message(MESSAGE)
        .add_file("submodules/repo_b", "File implicitly deleting submodule")
        .commit()
        .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    println!("large_repo_cs_id: {}", large_repo_cs_id);

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;
    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

    let expected_cs_id =
        ChangesetId::from_str("b0db847efd159d8c84d9227c6ae2ac74caee7ff0c07543c034472b596f1af52c")
            .unwrap();

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(expected_cs_id)).await;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[ExpectedChangeset::new_by_file_change(
            MESSAGE,
            // Add a regular file in the same path as the submodule expansion
            vec!["repo_a/submodules/repo_b"],
            // Files being deleted
            vec![
                // The submodule metadata file should also be deleted
                // TODO(T179534458): delete metadata file when submodule is implicitly deleted
                // "repo_a/submodules/.x-repo-submodule-repo_b",

                // The submodule expansion should be entirely deleted
                // TODO(T179534458): properly support submodule implicit deletion
                // "repo_a/submodules/repo_b/B_A",
                // "repo_a/submodules/repo_b/B_B",
            ],
        )],
    )?;
    Ok(())
}

/// Implicitly deleting files in the submodule repo (repo_b) should generate the
/// proper deletions in its expansion.
#[fbinit::test]
async fn test_implicit_deletions_inside_submodule_repo(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        repo_b_info: (repo_b, repo_b_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(fb, vec![]).await?;

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

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_id)
        .await?;

    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    // Update repo B submodule pointer in repo A to point to the last commit
    // with the implicit deletions.
    const MESSAGE: &str = "Update submodule after implicit deletions";
    let cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![*repo_a_cs_map.get("A_C").unwrap()])
        .set_message(MESSAGE)
        .add_file_with_type(
            "submodules/repo_b",
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let _large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;
    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

    let expected_cs_id =
        ChangesetId::from_str("7fc76cfa1906ccc74f86322cf529c5508867b5cfef8b80fb65a425e835f4b92b")
            .unwrap();

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(expected_cs_id)).await;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[ExpectedChangeset::new_by_file_change(
            MESSAGE,
            // Submodule metadata file is updated
            vec![
                "repo_a/submodules/.x-repo-submodule-repo_b",
                "repo_a/submodules/repo_b/some_dir",
            ],
            // Files being implicitly deleted
            // TODO(T179534458): properly support submodule implicit deletion
            vec![
                // "repo_a/submodules/repo_b/some_dir/file_x",
                // "repo_a/submodules/repo_b/some_dir/file_y"
            ],
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

/// Test adding a submodule dependency in the source repo in the path of an existing
/// file. This should generate a deletion of the file in the large repo, along
/// with the expansion of the submodule.
#[fbinit::test]
async fn test_implicitly_deleting_file_with_submodule(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    // Create repo C, to be added as a submodule in repo A.
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        repo_b_info: (_repo_b, _repo_b_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        // Add it as a submdule in the path of an existing file.
        vec![(NonRootMPath::new("A_A").unwrap(), repo_c.clone())],
    )
    .await?;

    let repo_c_mapped_git_commit = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_B"])
        .await?;

    let repo_c_git_commit_hash = *repo_c_mapped_git_commit.oid();

    const MESSAGE: &str = "Add submodule on path of existing file";
    let cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_map["A_C"]])
        .set_message(MESSAGE)
        .add_file_with_type(
            "A_A",
            repo_c_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let _large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await?;

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;

    derive_all_data_types_for_repo(&ctx, &large_repo, large_repo_changesets.as_slice()).await?;

    let expected_cs_id =
        ChangesetId::from_str("a586b2e4b85ef2ab37aa37a78711d82a10733098975c2ea352f3d80729d5cd6f")
            .unwrap();

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(expected_cs_id)).await;

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[ExpectedChangeset::new_by_file_change(
            MESSAGE,
            vec![
                "repo_a/.x-repo-submodule-A_A",
                "repo_a/A_A/C_A",
                "repo_a/A_A/C_B",
            ],
            vec![
                // The original file is deleted because of the submodule expansion
                "repo_a/A_A",
            ],
        )],
    )?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        expected_cs_id,
        NonRootMPath::new("repo_a/.x-repo-submodule-A_A")?,
        &repo_c_git_commit_hash,
    )
    .await?;

    Ok(())
}

/// Test adding a submodule dependency in the source repo in the path of an
/// existing **directory**. This should generate a deletion for all the files
/// in the directory, along with the expansion of the submodule.
#[fbinit::test]
async fn test_adding_submodule_on_existing_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    // Create repo C, to be added as a submodule in repo A.
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let dir_path = NonRootMPath::new("some_dir/subdir")?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        repo_b_info: (_repo_b, _repo_b_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        // Add it as a submdule in the path of an existing directory.
        vec![(dir_path.clone(), repo_c.clone())],
    )
    .await?;

    const ADD_DIR_MSG: &str = "Create directory with a few files";
    let add_dir_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_map["A_C"]])
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
    let _ = sync_to_master(ctx.clone(), &commit_syncer, add_dir_cs_id).await?;

    let repo_c_mapped_git_commit = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_B"])
        .await?;

    let repo_c_git_commit_hash = *repo_c_mapped_git_commit.oid();

    const MESSAGE: &str = "Add submodule on path of existing directory";
    let cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![add_dir_cs_id])
        .set_message(MESSAGE)
        .add_file_with_type(
            dir_path,
            repo_c_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let _large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await?;

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;

    derive_all_data_types_for_repo(&ctx, &large_repo, large_repo_changesets.as_slice()).await?;

    let expected_cs_id =
        ChangesetId::from_str("349e55c3d49f9c4841e218c49fb3bee3e6ea29fa8a95774df22f7cd307a109ad")
            .unwrap();

    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[
            ExpectedChangeset::new_by_file_change(
                ADD_DIR_MSG,
                vec![
                    "repo_a/some_dir/subdir/file_x",
                    "repo_a/some_dir/subdir/file_y",
                    "repo_a/some_dir/subdir/file_z",
                    "repo_a/some_dir/subdir/C_A",
                ],
                vec![],
            ),
            ExpectedChangeset::new_by_file_change(
                MESSAGE,
                vec![
                    "repo_a/some_dir/.x-repo-submodule-subdir",
                    "repo_a/some_dir/subdir/C_A",
                    "repo_a/some_dir/subdir/C_B",
                ],
                vec![
                    // All files from the directory should be deleted with
                    // the addition of a submodule expansion on the same path
                    "repo_a/some_dir/subdir/file_x",
                    "repo_a/some_dir/subdir/file_y",
                    "repo_a/some_dir/subdir/file_z",
                    // NOTE: We DON'T actually want a deletion for C_A, because
                    // the submodule expansion has the file with the same path.
                    // I'm leaving this commented out to convey this clearly.
                    // "repo_a/some_dir/subdir/C_A",
                ],
            ),
        ],
    )?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        expected_cs_id,
        NonRootMPath::new("repo_a/some_dir/.x-repo-submodule-subdir")?,
        &repo_c_git_commit_hash,
    )
    .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(expected_cs_id)).await;

    Ok(())
}

// ------------------ Unexpected state / Error handling ------------------

// TODO(T181473986): modify expansion manually without updating metadata file
// and assert that sync fails.

// TODO(T179533620): test that modifying submodule metadata file manually without
// the correct changes to the expansion will fail the sync.

// TODO(T179533620): test that deleting submodule metadata file manually
// fails the sync

/// Test that sync fails if submodule dependency repo is not available.
#[fbinit::test]
async fn test_submodule_expansion_crashes_when_dep_not_available(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    // Create repo C, to be added as a submodule in repo A.
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        repo_b_info: (_repo_b, _repo_b_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        // Don't pass repo C as a submodule dependency of repo A
        vec![],
    )
    .await?;

    // Get a git commit from repo C
    let repo_c_mapped_git_commit = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_B"])
        .await?;

    let repo_c_git_commit_hash = *repo_c_mapped_git_commit.oid();

    // Create a commit in repo A that adds repo C as a submodule.
    const MESSAGE: &str = "Add submodule on path of existing file";
    let cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_map["A_C"]])
        .set_message(MESSAGE)
        .add_file_with_type(
            "submodules/repo_c",
            repo_c_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await;

    assert!(sync_result.is_err_and(|e| {
        e.to_string()
            .contains("Mononoke repo from submodule submodules/repo_c not available")
    }));

    // Get all the changesets in the large repo
    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;

    derive_all_data_types_for_repo(&ctx, &large_repo, large_repo_changesets.as_slice()).await?;

    // And confirm that nothing was synced, i.e. all changesets are from the basic
    // setup.
    compare_expected_changesets_from_basic_setup(&large_repo_changesets, &[])?;

    Ok(())
}
