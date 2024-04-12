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
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use context::CoreContext;
use fbinit::FacebookInit;
use git_types::MappedGitCommitId;
use maplit::btreemap;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
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
#[fbinit::test]
async fn test_submodule_expansion_basic(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await?;

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
                REPO_B_SUBMODULE_PATH,
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
/// Tests the basic setup of expanding submodules that contain other submodules.
#[fbinit::test]
async fn test_recursive_submodule_expansion_basic(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_git_sha1 = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_B"])
        .await?;

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) = build_repo_b_with_c_submodule(
        fb,
        *c_master_git_sha1.oid(),
        &repo_c_submodule_path_in_repo_b,
    )
    .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);
    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            (repo_c_submodule_path, repo_c.clone()),
        ],
    )
    .await?;

    let master_before_change = large_repo
        .bookmarks()
        .get(ctx.clone(), &BookmarkKey::new(MASTER_BOOKMARK_NAME)?)
        .await?
        .ok_or(anyhow!(
            "Failed to get master bookmark changeset id of repo {}",
            large_repo.repo_identity().name()
        ))?;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        master_before_change,
        vec![
            "large_repo_root",
            "repo_a/A_A",
            "repo_a/A_B",
            "repo_a/A_C",
            "repo_a/submodules/.x-repo-submodule-repo_b",
            "repo_a/submodules/repo_b/B_A",
            "repo_a/submodules/repo_b/B_B",
            // TODO(T174902563): expect metadata file for recursive submodule
            // "repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
            "repo_a/submodules/repo_b/submodules/repo_c/C_A",
            "repo_a/submodules/repo_b/submodules/repo_c/C_B",
        ],
    )
    .await?;

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

    let repo_a_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_map["A_C"]])
        .set_message(MESSAGE)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, repo_a_cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;

    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

    let expected_cs_id =
        ChangesetId::from_str("f2da69f7deabd3e04683344884cb786a8adc625663074d24566258855e8767ce")
            .unwrap();

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        expected_cs_id,
        NonRootMPath::new("repo_a/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    // TODO(T174902563): check for repo_c submodule metadata file.
    assert!(
        check_submodule_metadata_file_in_large_repo(
            &ctx,
            &large_repo,
            expected_cs_id,
            NonRootMPath::new("repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c")?,
            &repo_b_git_commit_hash,
        )
        .await
        .is_err_and(|e| e
            .to_string()
            .contains("No fsnode entry for x-repo submodule metadata"))
    );

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "repo_a/A_A",
            "repo_a/A_B",
            "repo_a/A_C",
            "repo_a/submodules/.x-repo-submodule-repo_b",
            "repo_a/submodules/repo_b/B_A",
            "repo_a/submodules/repo_b/new_dir/new_file",
            // TODO(T174902563): expect metadata file for recursive submodule
            // "repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
            "repo_a/submodules/repo_b/submodules/repo_c/C_A",
            "repo_a/submodules/repo_b/submodules/repo_c/C_B",
        ],
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        repo_a_cs_id,
        Some(expected_cs_id),
    )
    .await;

    Ok(())
}

// ------------------------- Deletions ----------------------------

/// Deleting an entire submodule in the small repo (i.e. repo_a) should delete
/// its expansion and its metadata file in repo_a folder in the large repo.
#[fbinit::test]
async fn test_submodule_deletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await?;

    const MESSAGE: &str = "Delete repo_b submodule in repo_a";
    let cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![*repo_a_cs_map.get("A_C").unwrap()])
        .set_message(MESSAGE)
        .delete_file(REPO_B_SUBMODULE_PATH)
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
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await?;

    const MESSAGE: &str = "Implicitly delete repo_b submodule in repo_a";

    let cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![*repo_a_cs_map.get("A_C").unwrap()])
        .set_message(MESSAGE)
        .add_file(REPO_B_SUBMODULE_PATH, "File implicitly deleting submodule")
        .commit()
        .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, cs_id)
        .await?
        .ok_or(anyhow!("Commit wasn't synced"))?;

    println!("large_repo_cs_id: {}", large_repo_cs_id);

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;
    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

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
                "repo_a/submodules/.x-repo-submodule-repo_b",
                // NOTE: no need to have explicit deletions for these files, because
                // they're being deleted implicitly.
                // "repo_a/submodules/repo_b/B_A",
                // "repo_a/submodules/repo_b/B_B",
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
            "repo_a/A_A",
            "repo_a/A_B",
            "repo_a/A_C",
            "repo_a/submodules/repo_b",
        ],
    )
    .await?;

    let expected_cs_id =
        ChangesetId::from_str("723b5fd70f5a429c35fef028b7165f63c7f94821493db9188d83f6a2603b91a5")
            .unwrap();

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(expected_cs_id)).await;
    Ok(())
}

/// Implicitly deleting files in the submodule repo (repo_b) should generate the
/// proper deletions in its expansion.
#[fbinit::test]
async fn test_implicit_deletions_inside_submodule_repo(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
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
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let large_repo_master = sync_to_master(ctx.clone(), &commit_syncer, cs_id)
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
            // NOTE: no need to have explicit deletions for these files, because
            // they're being deleted implicitly.
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

    // Assert that `file_x` and `file_y` are not in the working copy
    // by getting all leaves from the RootFsnode
    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_master,
        vec![
            "large_repo_root",
            "repo_a/A_A",
            "repo_a/A_B",
            "repo_a/A_C",
            "repo_a/submodules/.x-repo-submodule-repo_b",
            "repo_a/submodules/repo_b/B_A",
            "repo_a/submodules/repo_b/B_B",
            "repo_a/submodules/repo_b/some_dir",
        ],
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
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    // Create repo C, to be added as a submodule in repo A.
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        mapping,
        live_commit_sync_config,
        test_sync_config_source,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        // Initial config should only have repo B as submodule dependency,
        // because the test data setup will create a file in the path `A_A`
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await?;

    // Update the commit syncer to use a new config version with extra submodule
    // dependencies.
    // This config version will include the submodule that will be added in the
    // submodule deps.
    let commit_syncer = add_new_commit_sync_config_version_with_submodule_deps(
        &ctx,
        &repo_a,
        &large_repo,
        "repo_a",
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            // Add it as a submdule in the path of the existing `A_A` file.
            (NonRootMPath::new("A_A").unwrap(), repo_c.clone()),
        ],
        mapping,
        live_commit_sync_config,
        test_sync_config_source,
    )?;

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
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    // Create repo C, to be added as a submodule in repo A.
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let dir_path = NonRootMPath::new("some_dir/subdir")?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        mapping,
        live_commit_sync_config,
        test_sync_config_source,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        // Add it as a submdule in the path of an existing directory.
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
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

    let _ = sync_to_master(ctx.clone(), &commit_syncer, add_dir_cs_id)
        .await
        .context("Failed to sync commit creating normal directory with files")?;

    // Update the commit syncer to use a new config version with extra submodule
    // dependencies.
    // This config version will include the submodule that will be added in the
    // path of an existing directory.
    let commit_syncer = add_new_commit_sync_config_version_with_submodule_deps(
        &ctx,
        &repo_a,
        &large_repo,
        "repo_a",
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            // Add the submodule path to the config
            (dir_path.clone(), repo_c.clone()),
        ],
        mapping,
        live_commit_sync_config,
        test_sync_config_source,
    )?;

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

    let _large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, cs_id)
        .await
        .context("Failed to sync commit replacing existing directory with submodule expansion")?;

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

/// Test that sync fails if submodule dependency repo is not available.
#[fbinit::test]
async fn test_submodule_expansion_crashes_when_dep_not_available(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    // Create repo C, to be added as a submodule in repo A.
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo,
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        // Don't pass repo C as a submodule dependency of repo A
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
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

    derive_all_data_types_for_repo(&ctx, &large_repo, large_repo_changesets.as_slice()).await?;

    // And confirm that nothing was synced, i.e. all changesets are from the basic
    // setup.
    compare_expected_changesets_from_basic_setup(&large_repo_changesets, &[])?;

    Ok(())
}
