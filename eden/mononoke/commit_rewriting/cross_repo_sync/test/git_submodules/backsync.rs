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
use blobstore::Loadable;
use context::CoreContext;
use fbinit::FacebookInit;
use git_types::MappedGitCommitId;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use tests_utils::CreateCommitContext;

use crate::git_submodules::git_submodules_test_utils::*;

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
        repo_a_info: (repo_a, _repo_a_cs_map),
        live_commit_sync_config,
        ..
    } = build_submodule_sync_test_data(
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
            "repo_a/submodules/.x-repo-submodule-repo_b",
            b_a_git_hash.to_string(),
        )
        // Delete the file added in commit B_B, to achieve working copy
        // equivalence with B_A
        .delete_file("repo_a/submodules/repo_b/B_B")
        .commit()
        .await
        .context("Failed to create commit modifying repo_a directory")?;
    let bonsai = cs_id.load(&ctx, large_repo.repo_blobstore()).await?;

    let validation_res = test_submodule_expansion_validation_in_large_repo_bonsai(
        ctx,
        bonsai,
        large_repo,
        repo_a,
        commit_syncer,
        live_commit_sync_config,
    )
    .await;

    assert!(
        validation_res.is_ok(),
        "Validation failed when working copy matches submodule pointer"
    );

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
        repo_a_info: (repo_a, _repo_a_cs_map),
        live_commit_sync_config,
        ..
    } = build_submodule_sync_test_data(
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
            "repo_a/submodules/repo_b/B_B",
            "Changing file in repo_a directory",
        )
        .commit()
        .await
        .context("Failed to create commit modifying repo_a directory")?;
    let bonsai = cs_id.load(&ctx, large_repo.repo_blobstore()).await?;

    let validation_res = test_submodule_expansion_validation_in_large_repo_bonsai(
        ctx,
        bonsai,
        large_repo,
        repo_a,
        commit_syncer,
        live_commit_sync_config,
    )
    .await;

    let expected_err_msg = "Expansion of submodule submodules/repo_b changed without updating its metadata file repo_a/submodules/.x-repo-submodule-repo_b";
    assert!(validation_res.is_err_and(|e| { e.to_string().contains(expected_err_msg) }));

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
        repo_a_info: (repo_a, _repo_a_cs_map),
        live_commit_sync_config,
        ..
    } = build_submodule_sync_test_data(
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
            "repo_a/submodules/.x-repo-submodule-repo_b",
            b_a_git_hash.to_string(),
        )
        .commit()
        .await
        .context("Failed to create commit modifying repo_a directory")?;
    let bonsai = cs_id.load(&ctx, large_repo.repo_blobstore()).await?;

    let validation_res = test_submodule_expansion_validation_in_large_repo_bonsai(
        ctx,
        bonsai,
        large_repo,
        repo_a,
        commit_syncer,
        live_commit_sync_config,
    )
    .await;

    let expected_err_msg = "Files present in the expansion are unaccounted for";
    assert!(validation_res.is_err_and(|e| { e.to_string().contains(expected_err_msg) }));

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
        repo_a_info: (repo_a, _repo_a_cs_map),
        live_commit_sync_config,
        ..
    } = build_submodule_sync_test_data(
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
            "repo_a/submodules/.x-repo-submodule-repo_b",
            c_a_git_hash.to_string(),
        )
        .commit()
        .await
        .context("Failed to create commit modifying repo_a directory")?;
    let bonsai = cs_id.load(&ctx, large_repo.repo_blobstore()).await?;

    let validation_res = test_submodule_expansion_validation_in_large_repo_bonsai(
        ctx,
        bonsai,
        large_repo,
        repo_a,
        commit_syncer,
        live_commit_sync_config,
    )
    .await;
    println!("Validation result: {0:#?}", &validation_res);

    let expected_err_msg = "Failed to get changeset id from git submodule commit hash 76ba5635bc159cfa5ac555d95974116bc94473f0 in repo repo_b";
    assert!(validation_res.is_err_and(|e| { e.to_string().contains(expected_err_msg) }));

    Ok(())
}

/// Deleting the submodule metadata file without deleting the expansion is a
/// valid scenario, e.g. when users delete a submodule but keep its static copy
/// in the repo as regular files.
#[fbinit::test]
async fn test_deleting_submodule_metadata_file_without_expansion_passes_validation(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());

    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await.context("Failed to build repo_b")?;

    let SubmoduleSyncTestData {
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        repo_a_info: (repo_a, _repo_a_cs_map),
        live_commit_sync_config,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;

    const MESSAGE: &str = "Delete submodule metadata file without deleting expansion";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .delete_file("repo_a/submodules/.x-repo-submodule-repo_b")
        .commit()
        .await
        .context("Failed to create commit modifying repo_a directory")?;
    let bonsai = cs_id.load(&ctx, large_repo.repo_blobstore()).await?;

    let validation_res = test_submodule_expansion_validation_in_large_repo_bonsai(
        ctx,
        bonsai,
        large_repo,
        repo_a,
        commit_syncer,
        live_commit_sync_config,
    )
    .await;

    assert!(
        validation_res.is_ok(),
        "Validation failed when working copy matches submodule pointer"
    );

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
        repo_a_info: (repo_a, _repo_a_cs_map),
        live_commit_sync_config,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await
    .context("Failed to build test data")?;

    const MESSAGE: &str = "Delete submodule expansion without deleting metadata file";
    let cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
        .set_message(MESSAGE)
        .delete_file("repo_a/submodules/repo_b/B_A")
        .delete_file("repo_a/submodules/repo_b/B_B")
        .commit()
        .await
        .context("Failed to create commit modifying repo_a directory")?;
    let bonsai = cs_id.load(&ctx, large_repo.repo_blobstore()).await?;

    let validation_res = test_submodule_expansion_validation_in_large_repo_bonsai(
        ctx,
        bonsai,
        large_repo,
        repo_a,
        commit_syncer,
        live_commit_sync_config,
    )
    .await;

    println!("Validation result: {0:#?}", &validation_res);

    let expected_err_msg = "Expansion of submodule submodules/repo_b changed without updating its metadata file repo_a/submodules/.x-repo-submodule-repo_b";
    assert!(validation_res.is_err_and(|e| { e.to_string().contains(expected_err_msg) }));

    Ok(())
}
