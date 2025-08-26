/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cached_config::ConfigStore;
use cached_config::ModificationTime;
use cached_config::TestSource;
use context::CoreContext;
use fbinit::FacebookInit;
use live_commit_sync_config::CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use pushredirect::TestPushRedirectionConfig;

macro_rules! is_error_kind {
    ($result_expression:expr, $( $pattern:pat_param )|+ $( if $guard: expr )?) => {
        match $result_expression {
            Ok(_) => false,
            Err(e) => match e.downcast_ref::<ErrorKind>() {
                $( Some($pattern) )|+ $( if $guard )? => true,
                _ => false
            }
        }
    }
}

mod all_changes;
mod all_simple;
mod current_simple;
mod push_redirection;

const EMPTY_COMMIT_SYNC_ALL: &str = r#"{
     "repos": {}
 }"#;

fn ensure_all_updated() {
    // This is copy-pasted from `cached_config`'s own
    // unit test
    thread::yield_now();
    thread::sleep(Duration::from_secs(1));
}

fn get_ctx_source_store_and_live_config(
    fb: FacebookInit,
    all_commit_syncs: &str,
) -> (
    CoreContext,
    Arc<TestSource>,
    ConfigStore,
    Arc<TestPushRedirectionConfig>,
    CfgrLiveCommitSyncConfig,
) {
    let ctx = CoreContext::test_mock(fb);
    let test_source = Arc::new(TestSource::new());
    test_source.insert_config(
        CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS,
        all_commit_syncs,
        ModificationTime::UnixTimestamp(0),
    );

    // We want to always refresh these paths in the test setting
    test_source.insert_to_refresh(CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS.to_string());

    let test_push_redirection_config = Arc::new(TestPushRedirectionConfig::new());

    let store = ConfigStore::new(test_source.clone(), Duration::from_millis(2), None);
    let live_commit_sync_config =
        CfgrLiveCommitSyncConfig::new(&store, test_push_redirection_config.clone()).unwrap();
    (
        ctx,
        test_source,
        store,
        test_push_redirection_config,
        live_commit_sync_config,
    )
}
