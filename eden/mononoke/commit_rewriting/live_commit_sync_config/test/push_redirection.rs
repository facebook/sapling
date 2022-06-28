/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cached_config::ModificationTime;
use fbinit::FacebookInit;
use live_commit_sync_config::*;
use mononoke_types::RepositoryId;

use crate::ensure_all_updated;
use crate::get_ctx_source_store_and_live_config;
use crate::EMPTY_PUSHREDIRECTOR;
use crate::EMTPY_COMMIT_SYNC_ALL;

const PUSHREDIRECTOR_PUBLIC_ENABLED: &str = r#"{
    "per_repo": {
        "1": {
            "draft_push": false,
            "public_push": true
        }
    }
}"#;

const PUSHREDIRECTOR_BOTH_ENABLED: &str = r#"{
    "per_repo": {
        "1": {
            "draft_push": true,
            "public_push": true
        }
    }
}"#;

#[fbinit::test]
async fn test_enabling_push_redirection(fb: FacebookInit) {
    let (_ctx, test_source, _store, live_commit_sync_config) =
        get_ctx_source_store_and_live_config(fb, EMPTY_PUSHREDIRECTOR, EMTPY_COMMIT_SYNC_ALL);
    let repo_1 = RepositoryId::new(1);

    // Enable push-redirection of public commits
    test_source.insert_config(
        CONFIGERATOR_PUSHREDIRECT_ENABLE,
        PUSHREDIRECTOR_PUBLIC_ENABLED,
        ModificationTime::UnixTimestamp(1),
    );

    // Check that push-redirection of public commits has been picked up
    ensure_all_updated();
    assert_eq!(
        live_commit_sync_config.push_redirector_enabled_for_draft(repo_1),
        false
    );
    assert_eq!(
        live_commit_sync_config.push_redirector_enabled_for_public(repo_1),
        true
    );

    // Enable push-redirection of public and draft commits
    test_source.insert_config(
        CONFIGERATOR_PUSHREDIRECT_ENABLE,
        PUSHREDIRECTOR_BOTH_ENABLED,
        ModificationTime::UnixTimestamp(2),
    );

    // Check that push-redirection of public and draft commits has been picked up
    ensure_all_updated();
    assert_eq!(
        live_commit_sync_config.push_redirector_enabled_for_draft(repo_1),
        true
    );
    assert_eq!(
        live_commit_sync_config.push_redirector_enabled_for_public(repo_1),
        true
    );
}
