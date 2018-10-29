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

extern crate bonsai_hg_mapping;
extern crate mercurial_types_mocks;
extern crate mononoke_types_mocks;

use futures::Future;

use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingEntry, ErrorKind, SqlBonsaiHgMapping,
                        SqlConstructors};
use mercurial_types_mocks::nodehash as hg;
use mercurial_types_mocks::repo::REPO_ZERO;
use mononoke_types_mocks::changesetid as bonsai;

fn add_and_get<M: BonsaiHgMapping>(mapping: M) {
    let entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };
    assert_eq!(
        true,
        mapping
            .add(entry.clone())
            .wait()
            .expect("Adding new entry failed")
    );
    assert_eq!(
        false,
        mapping
            .add(entry.clone())
            .wait()
            .expect("Adding same entry failed")
    );

    let result = mapping
        .get(REPO_ZERO, hg::ONES_CSID.into())
        .wait()
        .expect("Get failed");
    assert_eq!(result, Some(entry.clone()));
    let result = mapping
        .get_hg_from_bonsai(REPO_ZERO, bonsai::ONES_CSID)
        .wait()
        .expect("Failed to get hg changeset by its bonsai counterpart");
    assert_eq!(result, Some(hg::ONES_CSID));
    let result = mapping
        .get_bonsai_from_hg(REPO_ZERO, hg::ONES_CSID)
        .wait()
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));

    let same_bc_entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::TWOS_CSID, // differ from entry.hg_cs_id
        bcs_id: bonsai::ONES_CSID,
    };
    let result = mapping
        .add(same_bc_entry.clone())
        .wait()
        .expect_err("Conflicting entries should haved produced an error");
    assert_matches!(
        result.downcast::<ErrorKind>(),
        Ok(ErrorKind::ConflictingEntries(ref e0, ref e1)) if e0 == &entry && e1 == &same_bc_entry
    );

    let same_hg_entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::TWOS_CSID, // differ from entry.bcs_id
    };
    let result = mapping
        .add(same_hg_entry.clone())
        .wait()
        .expect_err("Conflicting entries should haved produced an error");
    assert_matches!(
        result.downcast::<ErrorKind>(),
        Ok(ErrorKind::ConflictingEntries(ref e0, ref e1)) if e0 == &entry && e1 == &same_hg_entry
    );
}

fn missing<M: BonsaiHgMapping>(mapping: M) {
    let result = mapping
        .get(REPO_ZERO, bonsai::ONES_CSID.into())
        .wait()
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert_eq!(result, None);
}

#[test]
fn test_add_and_get() {
    async_unit::tokio_unit_test(|| {
        add_and_get(SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap());
    });
}

#[test]
fn test_missing() {
    async_unit::tokio_unit_test(|| {
        missing(SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap());
    });
}
