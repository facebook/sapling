/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use fbinit::FacebookInit;
use live_commit_sync_config::*;
use mononoke_macros::mononoke;
use mononoke_types::RepositoryId;
use pushredirect::PushRedirectionConfig;

use crate::ensure_all_updated;
use crate::get_ctx_source_store_and_live_config;
use crate::EMTPY_COMMIT_SYNC_ALL;

#[mononoke::fbinit_test]
async fn test_enabling_push_redirection(fb: FacebookInit) -> Result<()> {
    let (ctx, _test_source, _store, test_push_redirection_config, live_commit_sync_config) =
        get_ctx_source_store_and_live_config(fb, EMTPY_COMMIT_SYNC_ALL);
    let repo_1 = RepositoryId::new(1);

    // Enable push-redirection of public commits
    test_push_redirection_config
        .set(&ctx, RepositoryId::new(1), false, true)
        .await?;

    // Check that push-redirection of public commits has been picked up
    ensure_all_updated();
    assert!(
        !live_commit_sync_config
            .push_redirector_enabled_for_draft(&ctx, repo_1)
            .await?
    );
    assert!(
        live_commit_sync_config
            .push_redirector_enabled_for_public(&ctx, repo_1)
            .await?
    );

    // Enable push-redirection of public and draft commits
    test_push_redirection_config
        .set(&ctx, RepositoryId::new(1), true, true)
        .await?;

    // Check that push-redirection of public and draft commits has been picked up
    ensure_all_updated();
    assert!(
        live_commit_sync_config
            .push_redirector_enabled_for_draft(&ctx, repo_1)
            .await?
    );
    assert!(
        live_commit_sync_config
            .push_redirector_enabled_for_public(&ctx, repo_1)
            .await?
    );

    Ok(())
}
