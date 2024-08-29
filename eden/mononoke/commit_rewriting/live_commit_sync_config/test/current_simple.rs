/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use fbinit::FacebookInit;
use live_commit_sync_config::LiveCommitSyncConfig;
use mononoke_macros::mononoke;
use mononoke_types::RepositoryId;

use crate::get_ctx_source_store_and_live_config;
use crate::EMTPY_COMMIT_SYNC_ALL;

#[mononoke::fbinit_test]
async fn test_empty_configs(fb: FacebookInit) -> Result<()> {
    let (ctx, _test_source, _store, _test_push_redirection_config, live_commit_sync_config) =
        get_ctx_source_store_and_live_config(fb, EMTPY_COMMIT_SYNC_ALL);
    let repo_1 = RepositoryId::new(1);
    assert!(
        !live_commit_sync_config
            .push_redirector_enabled_for_draft(&ctx, repo_1)
            .await?
    );
    assert!(
        !live_commit_sync_config
            .push_redirector_enabled_for_public(&ctx, repo_1)
            .await?
    );

    Ok(())
}
