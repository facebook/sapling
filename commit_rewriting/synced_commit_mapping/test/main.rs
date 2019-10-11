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
use maplit::btreemap;

use context::CoreContext;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::{REPO_ONE, REPO_TWO, REPO_ZERO};
use synced_commit_mapping::{
    SqlConstructors, SqlSyncedCommitMapping, SyncedCommitMapping, SyncedCommitMappingEntry,
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

fn get_all<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let entry =
        SyncedCommitMappingEntry::new(REPO_ZERO, bonsai::ONES_CSID, REPO_ONE, bonsai::TWOS_CSID);
    mapping
        .add(ctx.clone(), entry.clone())
        .wait()
        .expect("Adding new entry failed");
    let entry =
        SyncedCommitMappingEntry::new(REPO_ZERO, bonsai::ONES_CSID, REPO_TWO, bonsai::THREES_CSID);
    mapping
        .add(ctx.clone(), entry.clone())
        .wait()
        .expect("Adding new entry failed");

    let result = mapping
        .get_all(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID)
        .wait()
        .expect("Get failed");
    assert_eq!(
        result,
        btreemap! {REPO_ONE => bonsai::TWOS_CSID, REPO_TWO => bonsai::THREES_CSID}
    );

    let result = mapping
        .get_all(ctx.clone(), REPO_ONE, bonsai::TWOS_CSID)
        .wait()
        .expect("Get failed");
    assert_eq!(result, btreemap! {REPO_ZERO => bonsai::ONES_CSID});
}

fn missing<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let result = mapping
        .get(ctx.clone(), REPO_ONE, bonsai::TWOS_CSID, REPO_ZERO)
        .wait()
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert_eq!(result, None);
}

fn get_multiple<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let entry12 =
        SyncedCommitMappingEntry::new(REPO_ZERO, bonsai::ONES_CSID, REPO_ONE, bonsai::TWOS_CSID);
    let entry34 =
        SyncedCommitMappingEntry::new(REPO_ZERO, bonsai::THREES_CSID, REPO_ONE, bonsai::FOURS_CSID);
    mapping
        .add(ctx.clone(), entry12.clone())
        .wait()
        .expect("Adding new entry failed");
    mapping
        .add(ctx.clone(), entry34.clone())
        .wait()
        .expect("Adding new entry failed");

    let query = vec![bonsai::ONES_CSID, bonsai::TWOS_CSID, bonsai::FIVES_CSID];

    let result = mapping
        .get_multiple(ctx.clone(), REPO_ZERO, REPO_ONE, &query[..])
        .wait()
        .expect("get_multiple failed");
    assert_eq!(result, vec![Some(bonsai::TWOS_CSID), None, None]);
    let result = mapping
        .get_multiple(ctx.clone(), REPO_ONE, REPO_ZERO, &query[..])
        .wait()
        .expect("get_multiple failed");
    assert_eq!(result, vec![None, Some(bonsai::ONES_CSID), None]);
}

fn find_first_synced_in_list<M: SyncedCommitMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let entry12 =
        SyncedCommitMappingEntry::new(REPO_ZERO, bonsai::ONES_CSID, REPO_ONE, bonsai::TWOS_CSID);
    let entry34 =
        SyncedCommitMappingEntry::new(REPO_ZERO, bonsai::THREES_CSID, REPO_ONE, bonsai::FOURS_CSID);
    mapping
        .add(ctx.clone(), entry12.clone())
        .wait()
        .expect("Adding new entry failed");
    mapping
        .add(ctx.clone(), entry34.clone())
        .wait()
        .expect("Adding new entry failed");

    let query = vec![bonsai::ONES_CSID, bonsai::TWOS_CSID, bonsai::FOURS_CSID];
    let result = mapping
        .find_first_synced_in_list(ctx.clone(), REPO_ZERO, REPO_ONE, &query[..])
        .wait()
        .expect("find_first_synced_in_list failed");
    assert_eq!(result, Some((bonsai::ONES_CSID, bonsai::TWOS_CSID)));
    let result = mapping
        .find_first_synced_in_list(ctx.clone(), REPO_ONE, REPO_ZERO, &query[..])
        .wait()
        .expect("find_first_synced_in_list failed");
    assert_eq!(result, Some((bonsai::TWOS_CSID, bonsai::ONES_CSID)));

    let query = vec![bonsai::SIXES_CSID, bonsai::FIVES_CSID, bonsai::THREES_CSID];
    let result = mapping
        .find_first_synced_in_list(ctx.clone(), REPO_ZERO, REPO_ONE, &query[..])
        .wait()
        .expect("find_first_synced_in_list failed");
    assert_eq!(result, Some((bonsai::THREES_CSID, bonsai::FOURS_CSID)));

    let result = mapping
        .find_first_synced_in_list(ctx.clone(), REPO_ONE, REPO_ZERO, &query[..])
        .wait()
        .expect("find_first_synced_in_list failed");
    assert_eq!(result, None);
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
fn test_get_all(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        get_all(fb, SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap())
    });
}

#[fbinit::test]
fn test_get_multiple(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        get_multiple(fb, SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap())
    })
}

#[fbinit::test]
fn test_find_first_synced_in_list(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        find_first_synced_in_list(fb, SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap())
    })
}
