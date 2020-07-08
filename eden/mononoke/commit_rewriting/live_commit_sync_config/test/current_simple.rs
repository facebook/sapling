/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use live_commit_sync_config::LiveCommitSyncConfig;
use mononoke_types::RepositoryId;

use crate::{
    get_ctx_source_store_and_live_config, CURRENT_COMMIT_SYNC_CONFIG_V1, EMPTY_PUSHREDIRECTOR,
    EMTPY_COMMMIT_SYNC_ALL, EMTPY_COMMMIT_SYNC_CURRENT,
};

#[fbinit::test]
fn test_empty_configs(fb: FacebookInit) {
    let (ctx, _test_source, _store, live_commit_sync_config) = get_ctx_source_store_and_live_config(
        fb,
        EMPTY_PUSHREDIRECTOR,
        EMTPY_COMMMIT_SYNC_CURRENT,
        EMTPY_COMMMIT_SYNC_ALL,
    );
    let repo_1 = RepositoryId::new(1);
    assert_eq!(
        live_commit_sync_config.push_redirector_enabled_for_draft(repo_1),
        false
    );
    assert_eq!(
        live_commit_sync_config.push_redirector_enabled_for_public(repo_1),
        false
    );
    assert!(live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_1)
        .is_err());
}

#[fbinit::test]
fn test_commit_sync_config_groups(fb: FacebookInit) {
    let (ctx, _test_source, _store, live_commit_sync_config) = get_ctx_source_store_and_live_config(
        fb,
        EMPTY_PUSHREDIRECTOR,
        CURRENT_COMMIT_SYNC_CONFIG_V1,
        EMTPY_COMMMIT_SYNC_ALL,
    );
    let repo_0 = RepositoryId::new(0);
    let repo_1 = RepositoryId::new(1);
    let repo_2 = RepositoryId::new(2);
    let repo_3 = RepositoryId::new(3);
    let repo_4 = RepositoryId::new(4);

    // CommitSyncConfig is accessible by the large repo id
    let csc_r0 = live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_0)
        .unwrap();
    // CommitSyncConfig is accessible by small repo ids
    let csc_r1 = live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_1)
        .unwrap();
    let csc_r2 = live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_2)
        .unwrap();

    // Same group, same configs
    assert_eq!(csc_r0, csc_r1);
    assert_eq!(csc_r1, csc_r2);

    let csc_r3 = live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_3)
        .unwrap();
    let csc_r4 = live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_4)
        .unwrap();
    // Same group, same configs
    assert_eq!(csc_r3, csc_r4);

    // Different groups, different configs
    assert_ne!(csc_r0, csc_r3);
}
