/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use live_commit_sync_config::LiveCommitSyncConfig;
use mononoke_types::RepositoryId;

use crate::get_ctx_source_store_and_live_config;
use crate::EMPTY_PUSHREDIRECTOR;
use crate::EMTPY_COMMIT_SYNC_ALL;

#[fbinit::test]
async fn test_empty_configs(fb: FacebookInit) {
    let (_ctx, _test_source, _store, live_commit_sync_config) =
        get_ctx_source_store_and_live_config(fb, EMPTY_PUSHREDIRECTOR, EMTPY_COMMIT_SYNC_ALL);
    let repo_1 = RepositoryId::new(1);
    assert_eq!(
        live_commit_sync_config.push_redirector_enabled_for_draft(repo_1),
        false
    );
    assert_eq!(
        live_commit_sync_config.push_redirector_enabled_for_public(repo_1),
        false
    );
}
