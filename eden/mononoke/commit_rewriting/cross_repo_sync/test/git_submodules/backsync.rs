/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_snake_case)]

//! Tests for handling git submodules in x-repo sync

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use git_types::MappedGitCommitId;
use mononoke_types::ChangesetId;
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
#[fbinit::test]
async fn test_changing_submodule_expansion_validation_passes_when_working_copy_matches(
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
        .commit()
        .await
        .context("Failed to create commit modifying small_repo directory")?;

    let small_repo_cs_id = sync_to_master(ctx.clone(), &commit_syncer, cs_id)
        .await?
        .ok_or(anyhow!("Failed to sync commit"))?;

    let small_repo_changesets = get_all_changeset_data_from_repo(&ctx, &small_repo).await?;

    println!("Small repo changesets: {0:#?}", &small_repo_changesets);

    derive_all_enabled_types_for_repo(&ctx, &small_repo, &small_repo_changesets).await?;

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

/// Test that if the submodule expansion is completely deleted along with its
/// submodule metadata file, the changeset deles the submodule in the small repo
/// when backsynced.
#[fbinit::test]
async fn test_backsyncing_full_submodule_expansion_deletion(fb: FacebookInit) -> Result<()> {
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

    let sync_result = sync_to_master(ctx.clone(), &commit_syncer, cs_id).await;

    // TODO(T179530927): support backsyncing submodule expansion deletion
    // For now, backsyncing will fail complaining about the change made to the
    // submodule metadata file.
    assert!(
        sync_result
            .is_err_and(|err| { err.to_string() == "Submodule metadata file change is invalid" })
    );

    // let small_repo_changesets = get_all_changeset_data_from_repo(&ctx, &small_repo).await?;\
    // println!("Small repo changesets: {0:#?}", &small_repo_changesets);
    // derive_all_enabled_types_for_repo(&ctx, &small_repo, &small_repo_changesets).await?;
    // check_mapping(ctx.clone(), &commit_syncer, cs_id, Some(small_repo_cs_id)).await;
    // compare_expected_changesets(
    //     small_repo_changesets.last_chunk::<1>().unwrap(),
    //     &[ExpectedChangeset::new(MESSAGE).with_deletions(vec![
    //         // Submodule being deleted
    //         "submodules/repo_b",
    //     ])],
    // )?;

    Ok(())
}

/// Test that backsync will crash for small repos with submodule expansion
/// enabled while backsyncing submodule changes is not properly supported.
#[fbinit::test]
async fn test_changing_submodule_expansion_without_metadata_file_fails_validation(
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
#[fbinit::test]
async fn test_changing_submodule_metadata_pointer_without_expansion_fails_validation(
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
#[fbinit::test]
async fn test_changing_submodule_metadata_pointer_to_git_commit_from_another_repo(
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
/// TODO(T187241943): don't allow users to backsync changesets where the metadat
/// file is deleted but the expansion is not.
#[fbinit::test]
async fn test_deleting_submodule_metadata_file_without_expansion_passes_validation(
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
        // TODO(T187241943): pass validation but fail backsyncing when user
        // only deletes the metadata file
        err.to_string() == "Submodule metadata file change is invalid"
    }));

    Ok(())
}

/// Test that manually deleting the submodule expansion without deleting the
/// metadata file fails validation
#[fbinit::test]
async fn test_deleting_submodule_expansion_without_metadata_file_fails_validation(
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
