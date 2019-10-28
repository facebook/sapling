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
