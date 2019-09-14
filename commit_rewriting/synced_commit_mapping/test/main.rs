// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests for the synced commits mapping.

#![deny(warnings)]

use fbinit::FacebookInit;
use futures::{future, lazy, Future};
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

#[fbinit::test]
fn test_add_and_get(fb: FacebookInit) {
    tokio::run(lazy(move || {
        future::ok(add_and_get(
            fb,
            SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap(),
        ))
    }));
}

#[fbinit::test]
fn test_missing(fb: FacebookInit) {
    tokio::run(lazy(move || {
        future::ok(missing(
            fb,
            SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap(),
        ))
    }));
}

#[fbinit::test]
fn test_get_all(fb: FacebookInit) {
    tokio::run(lazy(move || {
        future::ok(get_all(
            fb,
            SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap(),
        ))
    }));
}
