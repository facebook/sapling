/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_snake_case)]

//! Tests for handling git submodules in x-repo sync

use anyhow::Result;
use context::CoreContext;
use cross_repo_sync::verify_working_copy;
use cross_repo_sync::verify_working_copy_with_version;
use cross_repo_sync::Source;
use cross_repo_sync::Target;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::NonRootMPath;
use tests_utils::CreateCommitContext;

use crate::git_submodules::git_submodules_test_utils::*;
const REPO_B_SUBMODULE_PATH: &str = "submodules/repo_b";

#[mononoke::fbinit_test]
async fn test_verify_working_copy_with_submodules(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (_small_repo, small_repo_cs_map),
        large_repo_info: (_large_repo, large_repo_master),
        commit_syncer,
        live_commit_sync_config,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await?;
    let small_repo_master = small_repo_cs_map.get("A_C").unwrap();
    verify_working_copy(
        &ctx,
        &commit_syncer,
        *small_repo_master,
        live_commit_sync_config.clone(),
    )
    .await?;
    verify_working_copy(
        &ctx,
        &commit_syncer.reverse()?,
        large_repo_master,
        live_commit_sync_config,
    )
    .await?;
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_verify_working_copy_with_submodules_simple_error_case(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb.clone());
    let (repo_b, _repo_b_cs_map) = build_repo_b(fb).await?;

    let SubmoduleSyncTestData {
        small_repo_info: (_small_repo, small_repo_cs_map),
        large_repo_info: (large_repo, large_repo_master),
        commit_syncer,
        live_commit_sync_config,
        ..
    } = build_submodule_sync_test_data(
        fb,
        &repo_b,
        vec![(NonRootMPath::new(REPO_B_SUBMODULE_PATH)?, repo_b.clone())],
    )
    .await?;

    const CHANGE_SUBMODULE_EXPANSION_CONTENTS: &str = "Change expansion contents";
    let large_repo_with_changed_expansion_csid =
        CreateCommitContext::new(&ctx, &large_repo, vec![large_repo_master])
            .set_message(CHANGE_SUBMODULE_EXPANSION_CONTENTS)
            .delete_file("small_repo/submodules/repo_b/B_A".to_string().as_str())
            .commit()
            .await?;
    let small_repo_master = small_repo_cs_map.get("A_C").unwrap();
    assert!(
        verify_working_copy_with_version(
            &ctx,
            &commit_syncer,
            Source(*small_repo_master),
            Target(large_repo_with_changed_expansion_csid),
            &base_commit_sync_version_name(),
            live_commit_sync_config,
        )
        .await
        .is_err()
    );
    Ok(())
}
