/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the synced commits mapping.

#![deny(warnings)]

use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;

use context::CoreContext;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::{REPO_ONE, REPO_ZERO};
use sql_construct::SqlConstruct;
use synced_commit_mapping::{
    EquivalentWorkingCopyEntry, SqlSyncedCommitMapping, SyncedCommitMapping,
    SyncedCommitMappingEntry, WorkingCopyEquivalence,
};

async fn add_and_get<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let version_name = CommitSyncConfigVersion("TEST_VERSION_NAME".to_string());
    let ctx = CoreContext::test_mock(fb);
    let entry = SyncedCommitMappingEntry::new(
        REPO_ZERO,
        bonsai::ONES_CSID,
        REPO_ONE,
        bonsai::TWOS_CSID,
        version_name.clone(),
    );
    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry.clone())
            .compat()
            .await
            .expect("Adding new entry failed")
    );
    assert_eq!(
        false,
        mapping
            .add(ctx.clone(), entry)
            .compat()
            .await
            .expect("Adding same entry failed")
    );

    let res = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .compat()
        .await
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(
            bonsai::TWOS_CSID,
            Some(version_name.clone()),
        ))
    );

    // insert again
    let entry = SyncedCommitMappingEntry::new(
        REPO_ZERO,
        bonsai::THREES_CSID,
        REPO_ONE,
        bonsai::FOURS_CSID,
        CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
    );
    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry.clone())
            .compat()
            .await
            .expect("Adding new entry failed")
    );

    let res = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ZERO, bonsai::THREES_CSID, REPO_ONE)
        .compat()
        .await
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(
            bonsai::FOURS_CSID,
            Some(version_name.clone()),
        ))
    );

    let result = mapping
        .get(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .compat()
        .await
        .expect("Get failed")
        .into_iter()
        .next()
        .expect("Unexpectedly, mapping is absent")
        .0;
    assert_eq!(result, bonsai::TWOS_CSID);
    let result = mapping
        .get(ctx.clone(), REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .compat()
        .await
        .expect("Get failed")
        .into_iter()
        .next()
        .expect("Unexpectedly, mapping is absent")
        .0;
    assert_eq!(result, bonsai::ONES_CSID);
}

async fn missing<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let result = mapping
        .get(ctx.clone(), REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .compat()
        .await
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert!(result.is_empty());
}

async fn equivalent_working_copy<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let version_name = CommitSyncConfigVersion("TEST_VERSION_NAME".to_string());
    let result = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .compat()
        .await
        .expect("Failed to fetch equivalent working copy (should succeed with None instead)");
    assert_eq!(result, None);

    let entry = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::ONES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: Some(bonsai::TWOS_CSID),
        version_name: Some(version_name.clone()),
    };
    let result = mapping
        .insert_equivalent_working_copy(ctx.clone(), entry.clone())
        .compat()
        .await
        .expect("Failed to insert working copy");
    assert_eq!(result, true);

    let result = mapping
        .insert_equivalent_working_copy(ctx.clone(), entry)
        .compat()
        .await
        .expect("Failed to insert working copy");
    assert_eq!(result, false);

    let res = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .compat()
        .await
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(
            bonsai::TWOS_CSID,
            Some(version_name)
        ))
    );

    let null_entry = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::THREES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: None,
        version_name: None,
    };

    let result = mapping
        .insert_equivalent_working_copy(ctx.clone(), null_entry)
        .compat()
        .await
        .expect("Failed to insert working copy");
    assert_eq!(result, true);

    let res = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ZERO, bonsai::THREES_CSID, REPO_ONE)
        .compat()
        .await
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(res, Some(WorkingCopyEquivalence::NoWorkingCopy(None)));

    let should_fail = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::THREES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: Some(bonsai::TWOS_CSID),
        version_name: None,
    };
    assert!(
        mapping
            .insert_equivalent_working_copy(ctx.clone(), should_fail)
            .compat()
            .await
            .is_err()
    );
}

#[fbinit::test]
async fn test_add_and_get(fb: FacebookInit) {
    add_and_get(fb, SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap()).await;
}

#[fbinit::test]
async fn test_missing(fb: FacebookInit) {
    missing(fb, SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap()).await
}

#[fbinit::test]
async fn test_equivalent_working_copy(fb: FacebookInit) {
    equivalent_working_copy(fb, SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap()).await
}
