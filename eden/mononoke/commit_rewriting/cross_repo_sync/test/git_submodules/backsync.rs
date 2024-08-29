/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_snake_case)]

//! Tests for handling git submodules in x-repo sync

use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use git_types::MappedGitCommitId;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_derived_data::RepoDerivedDataRef;
use tests_utils::CreateCommitContext;

use crate::check_mapping;
use crate::git_submodules::git_submodules_test_utils::*;
use crate::sync_to_master;
use crate::TestRepo;

const REPO_B_SUBMODULE_PATH: &str = "submodules/repo_b";

// ------------------ Submodule expansion validation ------------------

/// Test that if a submodule expansion is updated to match a certain commit from
/// the submodule repo and the metadata file has that git commit, validation
/// passes.
#[mononoke::fbinit_test]
async fn test_valid_submodule_expansion_update_succeeds(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await.context("Failed to build repo_b")?;

    let SubmoduleSyncTestData {
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        small_repo_info: (small_repo, _small_repo_cs_map),
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;
    let b_a_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_map["B_A"])
        .await?;

    let b_a_git_hash = *b_a_mapped_git_commit.oid();
    const MESSAGE: &str = "Update git commit in submodule metadata file";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .add_file(
            "small_repo/submodules/.x-repo-submodule-repo_b",
            b_a_git_hash.to_string(),
        )
        // Delete the file added in commit B_B, to achieve working copy
        // equivalence with B_A
        .delete_file("small_repo/submodules/repo_b/B_B")
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let (small_repo_cs_id, small_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &small_repo, &commit_syncer)
            .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(small_repo_cs_id)).await;

    compare_expected_changesets(
        small_repo_changesets.last_chunk::<1>().unwrap(),
        &[ExpectedChangeset::new(MESSAGE).with_git_submodules(vec![
            // GitSubmodule file change
            "submodules/repo_b",
        ])],
    )?;

    Ok(())
}

/// Test that updating the submodule expansion of a recursive submodule (thus,
/// updating the expansion of the parent submodule) backsyncs successfully.
#[mononoke::fbinit_test]
async fn test_valid_recursive_submodule_expansion_update_succeeds(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_git_sha1 = git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"])
        .await
        .context("c_master")?;

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;

    let repo_c_rec_sm_path_in_repo_a =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);

    // Build repo_b with repo_c as submodule, pointing to commit C_B
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await
            .context("Failed to build repo_b")?;

    // Get git hash of commit C_A
    let c_a_git_sha1 = git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_A"])
        .await
        .context("c_a")?;

    // Create a changeset in repo_b that updates the repo_c submodule pointer to
    // commit C_A
    const MESSAGE_REPO_B: &str = "Update repo_c submodule pointer in repo_b";
    let repo_b_cs_id = CreateCommitContext::new(&ctx, &repo_b, vec![repo_b_cs_map["B_B"]])
        .set_message(MESSAGE_REPO_B)
        // Update repo_c submodule pointer in repo_b
        .add_file_with_type(
            repo_c_submodule_path_in_repo_b,
            c_a_git_sha1.into_inner(),
            FileType::GitSubmodule,
        )
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    // Save the git hash of that repo_b commit, to use it in its submodule
    // expansion
    let repo_b_git_sha1 = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id)
        .await
        .context("repo_b_cs_id")?;

    // Build small and large repos
    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, _small_repo_cs_map),
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            (repo_c_rec_sm_path_in_repo_a, repo_c.clone()),
        ],
    )
    .await?;

    // Create a commit in large repo, updating the submodule expansion of repos
    // B and C.
    // Update repo_b pointer to the commit where it updates its repo_c submodule
    // pointer.
    const MESSAGE: &str = "Update submodules repo_b and repo_c git pointers";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        // Update repo_c submodule pointer in repo_b expansion to commit C_A
        .add_file(
            "small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c",
            c_a_git_sha1.to_string(),
        )
        // Delete file to bring repo_c expansion working copy to commit C_A in
        // repo_C.
        .delete_file("small_repo/submodules/repo_b/submodules/repo_c/C_B")
        // Also update repo_b submodule pointer
        .add_file(
            "small_repo/submodules/.x-repo-submodule-repo_b",
            repo_b_git_sha1.to_string(),
        )
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let (small_repo_cs_id, small_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &small_repo, &commit_syncer)
            .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(small_repo_cs_id)).await;

    compare_expected_changesets(
        small_repo_changesets.last_chunk::<1>().unwrap(),
        &[ExpectedChangeset::new(MESSAGE).with_git_submodules(vec![
            // Expect only a repo_b git submodule change
            "submodules/repo_b",
        ])],
    )?;

    Ok(())
}

// TODO(T182967556): unit test for multiple valid recursive submodule updates

/// Test that if the submodule expansion is completely deleted along with its
/// submodule metadata file, the changeset deles the submodule in the small repo
/// when backsynced.
#[mononoke::fbinit_test]
async fn test_full_submodule_expansion_deletion_succeeds(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await.context("Failed to build repo_b")?;

    let SubmoduleSyncTestData {
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        small_repo_info: (small_repo, _small_repo_cs_map),
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;

    const MESSAGE: &str = "Update git commit in submodule metadata file";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .delete_file("small_repo/submodules/.x-repo-submodule-repo_b")
        // Delete all files from repo_b submodule expansion.
        .delete_file("small_repo/submodules/repo_b/B_A")
        .delete_file("small_repo/submodules/repo_b/B_B")
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let (small_repo_cs_id, small_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &small_repo, &commit_syncer)
            .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(small_repo_cs_id)).await;
    compare_expected_changesets(
        small_repo_changesets.last_chunk::<1>().unwrap(),
        &[ExpectedChangeset::new(MESSAGE).with_deletions(vec![
            // Submodule being deleted
            "submodules/repo_b",
        ])],
    )?;

    Ok(())
}

/// Test valid recursive submodule deletions backsync successfully
#[mononoke::fbinit_test]
async fn test_valid_recursive_submodule_expansion_deletion_succeeds(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await?;
    let c_master_git_sha1 = git_sha1_from_changeset(&ctx, &repo_c, repo_c_cs_map["C_B"])
        .await
        .context("c_master")?;

    let repo_c_submodule_path_in_repo_b = NonRootMPath::new("submodules/repo_c")?;

    let repo_c_rec_sm_path_in_repo_a =
        NonRootMPath::new(REPO_B_SUBMODULE_PATH)?.join(&repo_c_submodule_path_in_repo_b);

    // Build repo_b with repo_c as submodule, pointing to commit C_B
    let (repo_b, repo_b_cs_map) =
        build_repo_b_with_c_submodule(fb, c_master_git_sha1, &repo_c_submodule_path_in_repo_b)
            .await
            .context("Failed to build repo_b")?;

    // Create a changeset in repo_b that updates the repo_c submodule pointer to
    // commit C_A
    const MESSAGE_REPO_B: &str = "Delete repo_c submodule in repo_b";
    let repo_b_cs_id = CreateCommitContext::new(&ctx, &repo_b, vec![repo_b_cs_map["B_B"]])
        .set_message(MESSAGE_REPO_B)
        // Delete repo_c submodule pointer in repo_b
        .delete_file(repo_c_submodule_path_in_repo_b)
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    // Save the git hash of that repo_b commit, to use it in its submodule
    // expansion
    let repo_b_git_sha1 = git_sha1_from_changeset(&ctx, &repo_b, repo_b_cs_id)
        .await
        .context("repo_b_cs_id")?;

    // Build small and large repos
    let SubmoduleSyncTestData {
        small_repo_info: (small_repo, _small_repo_cs_map),
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![
            (NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone()),
            (repo_c_rec_sm_path_in_repo_a, repo_c.clone()),
        ],
    )
    .await?;

    // Create a commit in large repo, updating the submodule expansion of repos
    // B and C.
    // Update repo_b pointer to the commit where it updates its repo_c submodule
    // pointer.
    const MESSAGE: &str = "Update submodules repo_b and repo_c git pointers";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        // Completely delete the repo_c recursive submodule expansion
        .delete_file("small_repo/submodules/repo_b/submodules/.x-repo-submodule-repo_c")
        .delete_file("small_repo/submodules/repo_b/submodules/repo_c/C_A")
        .delete_file("small_repo/submodules/repo_b/submodules/repo_c/C_B")
        // Also update repo_b submodule pointer
        .add_file(
            "small_repo/submodules/.x-repo-submodule-repo_b",
            repo_b_git_sha1.to_string(),
        )
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let (small_repo_cs_id, small_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &small_repo, &commit_syncer)
            .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(small_repo_cs_id)).await;

    compare_expected_changesets(
        small_repo_changesets.last_chunk::<1>().unwrap(),
        &[ExpectedChangeset::new(MESSAGE).with_git_submodules(vec![
            // Expect only a repo_b git submodule change
            "submodules/repo_b",
        ])],
    )?;

    Ok(())
}

// TODO(T182967556): test changing one submodule and deleting another

/// Test that updating submodule expansions and changing small and large repo
/// files in the same commit backsyncs successfully and that **the small repo
/// commit is lossy**, i.e. it only contains the changes made to the small repo.
#[mononoke::fbinit_test]
async fn test_atomic_submodule_updates_with_other_changes_backsync_successfully(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await.context("Failed to build repo_b")?;

    let SubmoduleSyncTestData {
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        small_repo_info: (small_repo, _small_repo_cs_map),
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;
    let b_a_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_map["B_A"])
        .await?;

    let b_a_git_hash = *b_a_mapped_git_commit.oid();
    const MESSAGE: &str = "Update git commit in submodule metadata file";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .add_file(
            "small_repo/submodules/.x-repo-submodule-repo_b",
            b_a_git_hash.to_string(),
        )
        // Delete the file added in commit B_B, to achieve working copy
        // equivalence with B_A
        .delete_file("small_repo/submodules/repo_b/B_B")
        .add_file("large_repo_root", "Change large repo file")
        .add_file("small_repo/A_C", "Change small repo file")
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let (small_repo_cs_id, small_repo_changesets) =
        sync_changeset_and_derive_all_types(ctx.clone(), cs_id, &small_repo, &commit_syncer)
            .await?;

    check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(small_repo_cs_id)).await;

    compare_expected_changesets(
        small_repo_changesets.last_chunk::<1>().unwrap(),
        &[ExpectedChangeset::new(MESSAGE)
            .with_git_submodules(vec![
                // GitSubmodule file change
                "submodules/repo_b",
            ])
            .with_regular_changes(vec![
                // Small repo file change
                "A_C",
            ])],
    )?;

    Ok(())
}

// TODO(T182967556): test updating 2 small repos with submodule expansions.

//
//
// ----------------------------------------------------------------------------
// ------------------------- Invalid changes ----------------------------------
//
// Test scenarios where backsyncing should fail

/// Test that backsync will crash for small repos with submodule expansion
/// enabled while backsyncing submodule changes is not properly supported.
#[mononoke::fbinit_test]
async fn test_changing_submodule_expansion_without_metadata_file_fails(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await.context("Failed to build repo_b")?;

    let SubmoduleSyncTestData {
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        small_repo_info: (_small_repo, _small_repo_cs_map),
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;

    const MESSAGE: &str = "Change submodule expansion without updating metadata file";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .add_file(
            "small_repo/submodules/repo_b/B_B",
            "Changing file in small_repo directory",
        )
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;
    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await;

    let expected_err_msg = concat!(
        "Expansion of submodule submodules/repo_b changed without updating ",
        "its metadata file small_repo/submodules/.x-repo-submodule-repo_b"
    );

    assert_backsync_validation_error(
        sync_result,
        vec![
            "Validation of submodule expansion failed",
            "Validation of submodule submodules/repo_b failed",
            expected_err_msg,
        ],
    );

    Ok(())
}

/// Test that manually changing the submodule pointer in the metadata file
/// without properly updating the working copy to match that commit will
/// fail validation.
#[mononoke::fbinit_test]
async fn test_changing_submodule_metadata_pointer_without_expansion_fails(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_b, repo_b_cs_map) = build_repo_b(fb).await.context("Failed to build repo_b")?;

    let SubmoduleSyncTestData {
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        small_repo_info: (_small_repo, _small_repo_cs_map),
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;
    let b_a_mapped_git_commit = repo_b
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_b_cs_map["B_A"])
        .await?;

    let b_a_git_hash = *b_a_mapped_git_commit.oid();
    const MESSAGE: &str = "Update git commit in submodule metadata file";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .add_file(
            "small_repo/submodules/.x-repo-submodule-repo_b",
            b_a_git_hash.to_string(),
        )
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await;

    let expected_err_msg = "Files present in expansion are unaccounted for";

    assert_backsync_validation_error(
        sync_result,
        vec![
            "Validation of submodule expansion failed",
            "Validation of submodule submodules/repo_b failed",
            expected_err_msg,
        ],
    );

    Ok(())
}

/// Test that setting the submodule pointer to a valid git commit hash that's
/// not present in the submodule repo fails validation.
#[mononoke::fbinit_test]
async fn test_changing_submodule_metadata_pointer_to_git_commit_from_another_repo_fails(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_c, repo_c_cs_map) = build_repo_c(fb).await.context("Failed to build repo_c")?;
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await.context("Failed to build repo_b")?;

    let SubmoduleSyncTestData {
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        small_repo_info: (_small_repo, _small_repo_cs_map),
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;

    let c_a_mapped_git_commit = repo_c
        .repo_derived_data()
        .derive::<MappedGitCommitId>(&ctx, repo_c_cs_map["C_A"])
        .await?;

    let c_a_git_hash = *c_a_mapped_git_commit.oid();
    const MESSAGE: &str = "Set repo_b submodule to point to repo_c commit";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .add_file(
            "small_repo/submodules/.x-repo-submodule-repo_b",
            c_a_git_hash.to_string(),
        )
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await;

    println!("Sync result: {0:#?}", &sync_result);

    let expected_err_msg = concat!(
        "Failed to get changeset id from git submodule ",
        "commit hash 76ba5635bc159cfa5ac555d95974116bc94473f0 in repo repo_b"
    );

    assert_backsync_validation_error(
        sync_result,
        vec![
            "Validation of submodule expansion failed",
            "Validation of submodule submodules/repo_b failed",
            "Failed to get submodule bonsai changeset id",
            expected_err_msg,
        ],
    );

    Ok(())
}

/// Deleting the submodule metadata file without deleting the expansion is a
/// valid scenario, e.g. when users delete a submodule but keep its static copy
/// in the repo as regular files.
#[mononoke::fbinit_test]
async fn test_deleting_submodule_metadata_file_without_expansion_passes_fails(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await.context("Failed to build repo_b")?;

    let SubmoduleSyncTestData {
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        small_repo_info: (_small_repo, _small_repo_cs_map),
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;

    const MESSAGE: &str = "Delete submodule metadata file without deleting expansion";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .delete_file("small_repo/submodules/.x-repo-submodule-repo_b")
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await;

    println!("Sync result: {0:#?}", &sync_result);

    assert!(sync_result.is_err_and(|err| {
        err.to_string() == "Submodule metadata file was deleted but 2 files in the submodule expansion were not."
    }));

    const MESSAGE_2: &str = "Delete submodule metadata file and partially delete expansion";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE_2)
        .delete_file("small_repo/submodules/.x-repo-submodule-repo_b")
        .delete_file("small_repo/submodules/repo_b/B_A")
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await;

    println!("Sync result: {0:#?}", &sync_result);

    assert!(sync_result.is_err_and(|err| {
        err.to_string() == "Submodule metadata file was deleted but 1 files in the submodule expansion were not."
    }));

    Ok(())
}

/// Test that manually deleting the submodule expansion without deleting the
/// metadata file fails validation
#[mononoke::fbinit_test]
async fn test_deleting_submodule_expansion_without_metadata_file_fails(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await.context("Failed to build repo_b")?;

    let SubmoduleSyncTestData {
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        small_repo_info: (_small_repo, _small_repo_cs_map),
        ..
    } = build_submodule_backsync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;

    const MESSAGE: &str = "Delete submodule expansion without deleting metadata file";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .delete_file("small_repo/submodules/repo_b/B_A")
        .delete_file("small_repo/submodules/repo_b/B_B")
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await;

    println!("Sync result: {0:#?}", &sync_result);

    let expected_err_msg = concat!(
        "Expansion of submodule submodules/repo_b changed without updating ",
        "its metadata file small_repo/submodules/.x-repo-submodule-repo_b"
    );

    assert_backsync_validation_error(
        sync_result,
        vec![
            "Validation of submodule expansion failed",
            "Validation of submodule submodules/repo_b failed",
            expected_err_msg,
        ],
    );

    Ok(())
}

/// Takes a Result that's expected to be a submodule expansion validation error
/// when backsyncing a changeset and assert it matches the
/// expectations (e.g. error message, contexts).
fn assert_backsync_validation_error(result: Result<Option<ChangesetId>>, expected_msgs: Vec<&str>) {
    assert!(result.is_err_and(|e| {
        let error_msgs = e.chain().map(|e| e.to_string()).collect::<Vec<_>>();
        println!("Error messages: {:#?}", error_msgs);
        error_msgs == expected_msgs
    }));
}

pub(crate) async fn build_submodule_backsync_test_data(
    fb: FacebookInit,
    repo_b: &TestRepo,
    // Add more small repo submodule dependencies for the test case
    submodule_deps: Vec<(NonRootMPath, TestRepo)>,
) -> Result<SubmoduleSyncTestData> {
    let test_data = build_submodule_sync_test_data(fb, repo_b, submodule_deps).await?;
    let reverse_syncer = test_data.commit_syncer.reverse()?;

    Ok(SubmoduleSyncTestData {
        commit_syncer: reverse_syncer,
        ..test_data
    })
}
