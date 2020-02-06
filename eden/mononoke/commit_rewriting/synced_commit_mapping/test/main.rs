/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Tests for the synced commits mapping.

#![deny(warnings)]

use fbinit::FacebookInit;
use futures::Future;

use context::CoreContext;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::{REPO_ONE, REPO_ZERO};
use synced_commit_mapping::{
    EquivalentWorkingCopyEntry, SqlConstructors, SqlSyncedCommitMapping, SyncedCommitMapping,
    SyncedCommitMappingEntry, WorkingCopyEquivalence,
};

fn add_and_get<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let entry =
        SyncedCommitMappingEntry::new(REPO_ZERO, bonsai::ONES_CSID, REPO_ONE, bonsai::TWOS_CSID);
    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry.clone())
            .wait()
            .expect("Adding new entry failed")
    );
    assert_eq!(
        false,
        mapping
            .add(ctx.clone(), entry)
            .wait()
            .expect("Adding same entry failed")
    );

    let res = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .wait()
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(bonsai::TWOS_CSID))
    );

    // insert again
    let entry =
        SyncedCommitMappingEntry::new(REPO_ZERO, bonsai::THREES_CSID, REPO_ONE, bonsai::FOURS_CSID);
    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry.clone())
            .wait()
            .expect("Adding new entry failed")
    );

    let res = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ZERO, bonsai::THREES_CSID, REPO_ONE)
        .wait()
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(bonsai::FOURS_CSID))
    );

    let result = mapping
        .get(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .wait()
        .expect("Get failed");
    assert_eq!(result, Some(bonsai::TWOS_CSID));
    let result = mapping
        .get(ctx.clone(), REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .wait()
        .expect("Get failed");
    assert_eq!(result, Some(bonsai::ONES_CSID));
}

fn missing<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let result = mapping
        .get(ctx.clone(), REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .wait()
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert_eq!(result, None);
}

fn equivalent_working_copy<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let result = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .wait()
        .expect("Failed to fetch equivalent working copy (should succeed with None instead)");
    assert_eq!(result, None);

    let entry = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::ONES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: Some(bonsai::TWOS_CSID),
    };
    let result = mapping
        .insert_equivalent_working_copy(ctx.clone(), entry.clone())
        .wait()
        .expect("Failed to insert working copy");
    assert_eq!(result, true);

    let result = mapping
        .insert_equivalent_working_copy(ctx.clone(), entry)
        .wait()
        .expect("Failed to insert working copy");
    assert_eq!(result, false);

    let res = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID, REPO_ONE)
        .wait()
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(
        res,
        Some(WorkingCopyEquivalence::WorkingCopy(bonsai::TWOS_CSID))
    );

    let null_entry = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::THREES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: None,
    };

    let result = mapping
        .insert_equivalent_working_copy(ctx.clone(), null_entry)
        .wait()
        .expect("Failed to insert working copy");
    assert_eq!(result, true);

    let res = mapping
        .get_equivalent_working_copy(ctx.clone(), REPO_ZERO, bonsai::THREES_CSID, REPO_ONE)
        .wait()
        .expect("get equivalent wc failed, should succeed");

    assert_eq!(res, Some(WorkingCopyEquivalence::NoWorkingCopy));

    let should_fail = EquivalentWorkingCopyEntry {
        large_repo_id: REPO_ZERO,
        large_bcs_id: bonsai::THREES_CSID,
        small_repo_id: REPO_ONE,
        small_bcs_id: Some(bonsai::TWOS_CSID),
    };
    assert!(mapping
        .insert_equivalent_working_copy(ctx.clone(), should_fail)
        .wait()
        .is_err());
}

#[fbinit::test]
fn test_add_and_get(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        add_and_get(fb, SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap())
    });
}

#[fbinit::test]
fn test_missing(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        missing(fb, SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap())
    });
}

#[fbinit::test]
fn test_equivalent_working_copy(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        equivalent_working_copy(fb, SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap())
    });
}
