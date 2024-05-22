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
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use context::CoreContext;
use fbinit::FacebookInit;
use git_types::MappedGitCommitId;
use maplit::btreemap;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
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
        large_repo_info: (large_repo, _large_repo_master),
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
    let c_master_mapped_git_commit = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_B"])
        .await?;
    let c_master_git_sha1 = *c_master_mapped_git_commit.oid();

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);
    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
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
            "repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
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
    println!("large_repo_changesets: {:#?}\n\n", &large_repo_changesets);

    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

    let expected_cs_id =
        ChangesetId::from_str("7b95de313bd54b4654e3aae74d5f444cd6db44504f6808cb7f138f42fc61f6e7")
            .unwrap();

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        expected_cs_id,
        NonRootMPath::new("repo_a/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        expected_cs_id,
        NonRootMPath::new("repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c")?,
        &c_master_git_sha1,
    )
    .await?;

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
            "repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
            "repo_a/submodules/repo_b/new_dir/new_file",
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
        large_repo_info: (large_repo, _large_repo_master),
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

/// Test that deleting a recursive submodule also deletes its metadata file.
#[fbinit::test]
async fn test_recursive_submodule_deletion(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_mapped_git_commit = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_B"])
        .await?;
    let c_master_git_sha1 = *c_master_mapped_git_commit.oid();

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);
    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
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
    )
    .await?;

    // Delete repo_c submodule in repo_b
    let repo_b_cs_id =
        CreateCommitContext::new(&ctx, &repo_b, vec![*repo_b_cs_map.get("B_B").unwrap()])
            .set_message("Add and delete file from repo_b")
            .delete_file(repo_c_submodule_path_in_repo_b)
            .commit()
            .await?;

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_id)
        .await?;
    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    const MESSAGE: &str = "Update submodule after deleting repo_c submodule in repo_b";

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
        .await
        .context("sync_to_master failed")
        .and_then(|res| res.ok_or(anyhow!("No commit was synced")))?;

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;
    println!("large_repo_changesets: {:#?}\n\n", &large_repo_changesets);

    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

    compare_expected_changesets(
        large_repo_changesets.last_chunk::<1>().unwrap(),
        &[ExpectedChangeset::new_by_file_change(
            MESSAGE,
            // repo_b submodule metadata file is updated
            vec!["repo_a/submodules/.x-repo-submodule-repo_b"],
            // Files being deleted
            vec![
                // NOTE: repo_c submodule metadata file has to be deleted too
                "repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                "repo_a/submodules/repo_b/submodules/repo_c/C_A",
                "repo_a/submodules/repo_b/submodules/repo_c/C_B",
            ],
        )],
    )?;

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
            "repo_a/submodules/repo_b/B_B",
        ],
    )
    .await?;

    let expected_cs_id =
        ChangesetId::from_str("7f8fbaec6112ac5e14bb4385d744fa1fea6c64c800f30c59c9c0ffca509c4e4c")
            .unwrap();

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        repo_a_cs_id,
        Some(expected_cs_id),
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
#[fbinit::test]
async fn test_deleting_submodule_but_keeping_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await?;

    const DELETE_METADATA_FILE_MSG: &str = "Delete repo_b submodule and keept its static copy";

    let del_md_file_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_map["A_C"]])
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

    println!("large_repo_cs_id: {0:#?}", large_repo_cs_id);

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "repo_a/A_A",
            "repo_a/A_B",
            "repo_a/A_C",
            // Files from the submodule are now regular files in the small repo
            "repo_a/submodules/repo_b/B_A",
            "repo_a/submodules/repo_b/B_B",
        ],
    )
    .await?;

    const CHANGE_SUBMODULE_PATH_MSG: &str = "Change static copy of repo_b";

    let chg_sm_path_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![del_md_file_cs_id])
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

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, chg_sm_path_cs_id)
        .await
        .context("sync_to_master failed")
        .and_then(|res| res.ok_or(anyhow!("No commit was synced")))?;

    println!("large_repo_cs_id: {0:#?}", large_repo_cs_id);

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;
    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;
    compare_expected_changesets_from_basic_setup(
        &large_repo_changesets,
        &[
            // Changeset that deletes the submodule metadata file
            ExpectedChangeset::new_by_file_change(
                DELETE_METADATA_FILE_MSG,
                // The submodule files are treated as regular file changes
                vec![
                    "repo_a/submodules/repo_b/B_A",
                    "repo_a/submodules/repo_b/B_B",
                ],
                // Only submodule metadata file is deleted
                vec!["repo_a/submodules/.x-repo-submodule-repo_b"],
            ),
            // Changeset that modifies files in the submodule path, which is
            // now a static copy of the submodule
            ExpectedChangeset::new_by_file_change(
                CHANGE_SUBMODULE_PATH_MSG,
                // The submodule files are treated as regular file changes
                vec![
                    "repo_a/submodules/repo_b/B_B",
                    "repo_a/submodules/repo_b/B_C",
                ],
                // Only submodule metadata file is deleted
                vec!["repo_a/submodules/repo_b/B_A"],
            ),
        ],
    )?;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "repo_a/A_A",
            "repo_a/A_B",
            "repo_a/A_C",
            // Files from the submodule are now regular files in the small repo
            "repo_a/submodules/repo_b/B_B",
            "repo_a/submodules/repo_b/B_C",
        ],
    )
    .await?;

    // Check mappings of both commits
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        del_md_file_cs_id,
        Some(
            ChangesetId::from_str(
                "6176e1966404d7da39e62f814ddc49181cb7a31f63a40504f76feada0a47bcf4",
            )
            .unwrap(),
        ),
    )
    .await;
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        chg_sm_path_cs_id,
        Some(
            ChangesetId::from_str(
                "d988d913f6b9f1e91d1fdc41d317cf2d6bed335774c2d5415e33eaa46d086442",
            )
            .unwrap(),
        ),
    )
    .await;

    Ok(())
}

/// Same scenario as `test_deleting_submodule_but_keeping_directory`, but with
/// a recursive submodule.
#[fbinit::test]
async fn test_deleting_recursive_submodule_but_keeping_directory(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_mapped_git_commit = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_B"])
        .await?;
    let c_master_git_sha1 = *c_master_mapped_git_commit.oid();

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
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

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, del_repo_c_md_file_cs_id)
        .await?;
    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    let del_md_file_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_map["A_C"]])
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

    println!("large_repo_cs_id: {0:#?}", large_repo_cs_id);

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "repo_a/A_A",
            "repo_a/A_B",
            "repo_a/A_C",
            // Files from the submodule are now regular files in the small repo
            "repo_a/submodules/.x-repo-submodule-repo_b",
            "repo_a/submodules/repo_b/B_A",
            "repo_a/submodules/repo_b/B_B",
            "repo_a/submodules/repo_b/submodules/repo_c/C_A",
            "repo_a/submodules/repo_b/submodules/repo_c/C_B",
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

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, chg_repo_c_copy_cs_id)
        .await?;
    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    let chg_sm_path_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![del_md_file_cs_id])
        .set_message(CHANGE_SUBMODULE_PATH_MSG)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, chg_sm_path_cs_id)
        .await
        .context("Failed to sync chg_sm_path_cs_id")
        .and_then(|res| res.ok_or(anyhow!("No commit was synced")))?;

    println!("large_repo_cs_id: {0:#?}", large_repo_cs_id);

    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;
    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

    compare_expected_changesets(
        large_repo_changesets.last_chunk::<2>().unwrap(),
        &[
            // Changeset that deletes the submodule metadata file
            ExpectedChangeset::new_by_file_change(
                DELETE_METADATA_FILE_MSG,
                // The submodule files are treated as regular file changes
                vec![
                    // repo_b submodule metadata file is updated
                    "repo_a/submodules/.x-repo-submodule-repo_b",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_A",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_B",
                ],
                // Only submodule metadata file is deleted
                vec!["repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c"],
            ),
            // Changeset that modifies files in the submodule path, which is
            // now a static copy of the submodule
            ExpectedChangeset::new_by_file_change(
                CHANGE_SUBMODULE_PATH_MSG,
                // The submodule files are treated as regular file changes
                vec![
                    // repo_b submodule metadata file is updated
                    "repo_a/submodules/.x-repo-submodule-repo_b",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_B",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_C",
                ],
                // Only submodule metadata file is deleted
                vec!["repo_a/submodules/repo_b/submodules/repo_c/C_A"],
            ),
        ],
    )?;

    assert_working_copy_matches_expected(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        vec![
            "large_repo_root",
            "repo_a/A_A",
            "repo_a/A_B",
            "repo_a/A_C",
            // Files from the submodule are now regular files in the small repo
            "repo_a/submodules/.x-repo-submodule-repo_b",
            "repo_a/submodules/repo_b/B_A",
            "repo_a/submodules/repo_b/B_B",
            "repo_a/submodules/repo_b/submodules/repo_c/C_B",
            "repo_a/submodules/repo_b/submodules/repo_c/C_C",
        ],
    )
    .await?;

    // Check mappings of both commits
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        del_md_file_cs_id,
        Some(
            ChangesetId::from_str(
                "727dc8e42f7886bfb3bd919d58724cff6c4b7d5ea42410184b4c0027e53d8c54",
            )
            .unwrap(),
        ),
    )
    .await;
    check_mapping(
        ctx.clone(),
        &commit_syncer,
        chg_sm_path_cs_id,
        Some(
            ChangesetId::from_str(
                "3b28131208374b55997843bdcacef567aa8b1bb09212d2d3168c30ef056dcd60",
            )
            .unwrap(),
        ),
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
#[fbinit::test]
async fn test_implicitly_deleting_submodule(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
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
        large_repo_info: (large_repo, _large_repo_master),
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
        large_repo_info: (large_repo, _large_repo_master),
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
        large_repo_info: (large_repo, _large_repo_master),
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
        large_repo_info: (large_repo, _large_repo_master),
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

/// Tests that validation fails if a user adds a file in the original repo
/// matching the path of a submodule metadata file.
///
/// It's an unlikely scenario, but we want to be certain of what would happen,
/// because users might, for example, manually copy directories from the large
/// repo to the git repo.
#[fbinit::test]
async fn test_submodule_validation_fails_with_file_on_metadata_file_path_in_small_repo(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
        large_repo_info: (large_repo, _large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await?;

    const MESSAGE_CS_1: &str =
        "Add file with same path as a submodule metadata file with random content";

    let repo_a_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_map["A_C"]])
        .set_message(MESSAGE_CS_1)
        .add_file(
            "submodules/.x-repo-submodule-repo_b",
            "File that should only exist in the large repo",
        )
        .commit()
        .await?;

    println!("Trying to sync changeset #1!");

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, repo_a_cs_id)
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
    // check_mapping(ctx.clone(), &commit_syncer, repo_a_cs_id, None).await;

    // Do the same thing, but adding a valid git commit has in the file
    // To see what happens if a user tries updating a submodule in a weird
    // unexpected way.
    const MESSAGE_CS_2: &str =
        "Add file with same path as a submodule metadata file with valid git commit hash";

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_map["B_A"])
        .await?;

    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    let repo_a_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_map["A_C"]])
        .set_message(MESSAGE_CS_2)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    println!("Trying to sync changeset #2!");
    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, repo_a_cs_id)
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
    // check_mapping(ctx.clone(), &commit_syncer, repo_a_cs_id, None).await;

    Ok(())
}

/// Similar to the test above, but adding a file that maps to a submodule
/// metadata file path of a recursive submodule.
#[fbinit::test]
async fn test_submodule_validation_fails_with_file_on_metadata_file_path_in_recursive_submodule(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_mapped_git_commit = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_B"])
        .await?;
    let c_master_git_sha1 = *c_master_mapped_git_commit.oid();

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;

    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);
    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
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

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_id)
        .await?;
    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    const MESSAGE: &str =
        "Update repo_b submodule after adding a file in the same place as a metadata file";

    let repo_a_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_map["A_C"]])
        .set_message(MESSAGE)
        .add_file_with_type(
            REPO_B_SUBMODULE_PATH,
            repo_b_git_commit_hash.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, repo_a_cs_id)
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
    // check_mapping(ctx.clone(), &commit_syncer, repo_a_cs_id, None).await;

    Ok(())
}

/// Test that expanding a **known** dangling submodule pointer works as expected.
/// A commit is created in the submodule repo with a README file informing that
/// the pointer didn't exist in the submodule.
/// The list of known dangling submodule pointers can be set in the small repo's
/// sync config.
#[fbinit::test]
async fn test_expanding_known_dangling_submodule_pointers(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_mapped_git_commit = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_B"])
        .await?;
    let c_master_git_sha1 = *c_master_mapped_git_commit.oid();

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await?;
    let repo_c_submodule_path =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);

    let SubmoduleSyncTestData {
        repo_a_info: (repo_a, repo_a_cs_map),
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
    )
    .await?;

    // COMMIT 1: set a dangling pointer in repo_b submodule
    const COMMIT_MSG_1: &str = "Set submodule pointer to known dangling pointer from config";

    // Test expanding a known dangling submodule pointer
    let repo_b_dangling_pointer = GitSha1::from_str(REPO_B_DANGLING_GIT_COMMIT_HASH)?;

    let repo_a_cs_id =
        CreateCommitContext::new(&ctx, &repo_a, vec![*repo_a_cs_map.get("A_C").unwrap()])
            .set_message(COMMIT_MSG_1)
            .add_file_with_type(
                REPO_B_SUBMODULE_PATH,
                repo_b_dangling_pointer.into_inner(),
                FileType::GitSubmodule,
            )
            .commit()
            .await?;

    let large_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, repo_a_cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    // Look for the README file and assert its content matches expectation
    let bonsai = large_repo_cs_id
        .load(&ctx, large_repo.repo_blobstore())
        .await
        .context("Failed to load bonsai in large repo")?;
    let readme_file_change = bonsai
        .file_changes_map()
        .get(&NonRootMPath::new("repo_a/submodules/repo_b/README.TXT")?)
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
        NonRootMPath::new("repo_a/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_dangling_pointer,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        repo_a_cs_id,
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

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_id)
        .await?;
    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    let repo_a_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_id])
        .set_message(COMMIT_MSG_2)
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

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("repo_a/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        repo_a_cs_id,
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

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_id)
        .await?;
    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    let repo_a_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_id])
        .set_message(COMMIT_MSG_3)
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

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("repo_a/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        repo_a_cs_id,
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

    let repo_b_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_id)
        .await?;
    let repo_b_git_commit_hash = *repo_b_mapped_git_commit.oid();

    let repo_a_cs_id = CreateCommitContext::new(&ctx, &repo_a, vec![repo_a_cs_id])
        .set_message(COMMIT_MSG_4)
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

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("repo_a/submodules/.x-repo-submodule-repo_b")?,
        &repo_b_git_commit_hash,
    )
    .await?;

    check_submodule_metadata_file_in_large_repo(
        &ctx,
        &large_repo,
        large_repo_cs_id,
        NonRootMPath::new("repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c")?,
        &c_master_git_sha1,
    )
    .await?;

    check_mapping(
        ctx.clone(),
        &commit_syncer,
        repo_a_cs_id,
        Some(large_repo_cs_id),
    )
    .await;

    // ------------------ Assertions / validations ------------------

    // Get all the changesets
    let large_repo_changesets = get_all_changeset_data_from_repo(&ctx, &large_repo).await?;

    // Ensure everything can be derived successfully
    derive_all_data_types_for_repo(&ctx, &large_repo, &large_repo_changesets).await?;

    compare_expected_changesets(
        large_repo_changesets.last_chunk::<4>().unwrap(),
        &[
            // COMMIT 1: Expansion of known dangling pointer
            ExpectedChangeset::new_by_file_change(
                COMMIT_MSG_1,
                vec![
                    // Submodule metadata file is updated
                    "repo_a/submodules/.x-repo-submodule-repo_b",
                    // README file is added with a message informing that this
                    // submodule pointer was dangling.
                    "repo_a/submodules/repo_b/README.TXT",
                ],
                // Should delete everything from previous expansion
                vec![
                    "repo_a/submodules/repo_b/B_A",
                    "repo_a/submodules/repo_b/B_B",
                    "repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_A",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_B",
                ],
            ),
            // COMMIT 2: Fix the dangling pointer
            ExpectedChangeset::new_by_file_change(
                COMMIT_MSG_2,
                vec![
                    // Submodule metadata file is updated
                    "repo_a/submodules/.x-repo-submodule-repo_b",
                    // Add back files from previous submodule pointer
                    "repo_a/submodules/repo_b/B_A",
                    "repo_a/submodules/repo_b/B_B",
                    "repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_A",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_B",
                    // Plus the new file added in the new valid pointer
                    "repo_a/submodules/repo_b/B_C",
                ],
                vec![
                    // Delete README file from dangling pointer expansion
                    "repo_a/submodules/repo_b/README.TXT",
                ],
            ),
            // COMMIT 3: Set dangling pointer in repo_c recursive submodule
            ExpectedChangeset::new_by_file_change(
                COMMIT_MSG_3,
                vec![
                    // Submodule metadata files are updated
                    "repo_a/submodules/.x-repo-submodule-repo_b",
                    "repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                    // README file is added with a message informing that this
                    // submodule pointer was dangling.
                    "repo_a/submodules/repo_b/submodules/repo_c/README.TXT",
                ],
                // Should delete everything from previous expansion
                vec![
                    "repo_a/submodules/repo_b/submodules/repo_c/C_A",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_B",
                ],
            ),
            // COMMIT 4: Fix dangling pointer in repo_c recursive submodule
            ExpectedChangeset::new_by_file_change(
                COMMIT_MSG_4,
                vec![
                    // Submodule metadata files are updated
                    "repo_a/submodules/.x-repo-submodule-repo_b",
                    "repo_a/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
                    // Add back files from expansion of commit C_B
                    "repo_a/submodules/repo_b/submodules/repo_c/C_A",
                    "repo_a/submodules/repo_b/submodules/repo_c/C_B",
                ],
                vec![
                    // Delete README file from dangling pointer expansion
                    "repo_a/submodules/repo_b/submodules/repo_c/README.TXT",
                ],
            ),
        ],
    )?;

    Ok(())
}
