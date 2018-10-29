// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests for the Changesets store.

#![deny(warnings)]

#[macro_use]
extern crate assert_matches;
extern crate async_unit;
extern crate failure_ext as failure;
extern crate futures;

extern crate changesets;
extern crate mercurial_types_mocks;
extern crate mononoke_types_mocks;

use futures::Future;

use changesets::{ChangesetEntry, ChangesetInsert, Changesets, ErrorKind, SqlChangesets,
                 SqlConstructors};
use mercurial_types_mocks::repo::*;
use mononoke_types_mocks::changesetid::*;

fn add_and_get<C: Changesets + 'static>(changesets: C) {
    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    changesets.add(row).wait().expect("Adding new entry failed");

    let result = changesets
        .get(REPO_ZERO, ONES_CSID)
        .wait()
        .expect("Get failed");
    assert_eq!(
        result,
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: ONES_CSID,
            parents: vec![],
            gen: 1,
        }),
    );
}

fn add_missing_parents<C: Changesets>(changesets: C) {
    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![TWOS_CSID],
    };
    let result = changesets
        .add(row)
        .wait()
        .expect_err("Adding entry with missing parents failed (should have succeeded)");
    assert_matches!(
        result.downcast::<ErrorKind>(),
        Ok(ErrorKind::MissingParents(ref x)) if x == &vec![TWOS_CSID]
    );
}

fn missing<C: Changesets + 'static>(changesets: C) {
    let result = changesets
        .get(REPO_ZERO, ONES_CSID)
        .wait()
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert_eq!(result, None);
}

fn duplicate<C: Changesets + 'static>(changesets: C) {
    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    assert_eq!(
        changesets
            .add(row.clone())
            .wait()
            .expect("Adding new entry failed"),
        true,
        "inserting unique changeset must return true"
    );

    assert_eq!(
        changesets
            .add(row)
            .wait()
            .expect("error while adding changeset"),
        false,
        "inserting the same changeset must return false"
    );
}

fn broken_duplicate<C: Changesets + 'static>(changesets: C) {
    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    assert_eq!(
        changesets.add(row).wait().expect("Adding new entry failed"),
        true,
        "inserting unique changeset must return true"
    );

    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    assert_eq!(
        changesets.add(row).wait().expect("Adding new entry failed"),
        true,
        "inserting unique changeset must return true"
    );

    let row = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![TWOS_CSID],
    };
    let result = changesets
        .add(row)
        .wait()
        .expect_err("Adding changeset with the same hash but differen parents should fail");
    match result.downcast::<ErrorKind>() {
        Ok(ErrorKind::DuplicateInsertionInconsistency(..)) => {}
        err => panic!("unexpected error: {:?}", err),
    };
}

fn complex<C: Changesets>(changesets: C) {
    let row1 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: ONES_CSID,
        parents: vec![],
    };
    changesets.add(row1).wait().expect("Adding row 1 failed");

    let row2 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    changesets.add(row2).wait().expect("Adding row 2 failed");

    let row3 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: THREES_CSID,
        parents: vec![TWOS_CSID],
    };
    changesets.add(row3).wait().expect("Adding row 3 failed");

    let row4 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: FOURS_CSID,
        parents: vec![ONES_CSID, THREES_CSID],
    };
    changesets.add(row4).wait().expect("Adding row 4 failed");

    let row5 = ChangesetInsert {
        repo_id: REPO_ZERO,
        cs_id: FIVES_CSID,
        parents: vec![ONES_CSID, TWOS_CSID, FOURS_CSID],
    };
    changesets.add(row5).wait().expect("Adding row 5 failed");

    assert_eq!(
        changesets
            .get(REPO_ZERO, ONES_CSID)
            .wait()
            .expect("Get row 1 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: ONES_CSID,
            parents: vec![],
            gen: 1,
        }),
    );

    assert_eq!(
        changesets
            .get(REPO_ZERO, TWOS_CSID)
            .wait()
            .expect("Get row 2 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: TWOS_CSID,
            parents: vec![],
            gen: 1,
        }),
    );

    assert_eq!(
        changesets
            .get(REPO_ZERO, THREES_CSID)
            .wait()
            .expect("Get row 3 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: THREES_CSID,
            parents: vec![TWOS_CSID],
            gen: 2,
        }),
    );

    assert_eq!(
        changesets
            .get(REPO_ZERO, FOURS_CSID)
            .wait()
            .expect("Get row 4 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: FOURS_CSID,
            parents: vec![ONES_CSID, THREES_CSID],
            gen: 3,
        }),
    );

    assert_eq!(
        changesets
            .get(REPO_ZERO, FIVES_CSID)
            .wait()
            .expect("Get row 5 failed"),
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: FIVES_CSID,
            parents: vec![ONES_CSID, TWOS_CSID, FOURS_CSID],
            gen: 4,
        }),
    );
}

#[test]
fn test_add_and_get() {
    async_unit::tokio_unit_test(|| {
        add_and_get(SqlChangesets::with_sqlite_in_memory().unwrap());
    });
}

#[test]
fn test_add_missing_parents() {
    async_unit::tokio_unit_test(|| {
        add_missing_parents(SqlChangesets::with_sqlite_in_memory().unwrap());
    });
}

#[test]
fn test_missing() {
    async_unit::tokio_unit_test(|| {
        missing(SqlChangesets::with_sqlite_in_memory().unwrap());
    });
}

#[test]
fn test_duplicate() {
    async_unit::tokio_unit_test(|| {
        duplicate(SqlChangesets::with_sqlite_in_memory().unwrap());
    });
}

#[test]
fn test_broken_duplicate() {
    async_unit::tokio_unit_test(|| {
        broken_duplicate(SqlChangesets::with_sqlite_in_memory().unwrap());
    });
}

#[test]
fn test_complex() {
    async_unit::tokio_unit_test(|| {
        complex(SqlChangesets::with_sqlite_in_memory().unwrap());
    });
}
