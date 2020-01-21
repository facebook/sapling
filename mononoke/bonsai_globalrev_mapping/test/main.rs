/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use bonsai_globalrev_mapping::{
    BonsaiGlobalrevMapping, BonsaiGlobalrevMappingEntry, BonsaisOrGlobalrevs,
    SqlBonsaiGlobalrevMapping, SqlConstructors,
};
use futures::Future;
use mercurial_types_mocks::globalrev::*;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;

fn add_and_get<M: BonsaiGlobalrevMapping>(mapping: M) {
    let entry = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };

    mapping
        .bulk_import(&vec![entry.clone()])
        .wait()
        .expect("Adding new entry failed");

    let result = mapping
        .get(
            REPO_ZERO,
            BonsaisOrGlobalrevs::Bonsai(vec![bonsai::ONES_CSID]),
        )
        .wait()
        .expect("Get failed");
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_globalrev_from_bonsai(REPO_ZERO, bonsai::ONES_CSID)
        .wait()
        .expect("Failed to get globalrev by its bonsai counterpart");
    assert_eq!(result, Some(GLOBALREV_ZERO));
    let result = mapping
        .get_bonsai_from_globalrev(REPO_ZERO, GLOBALREV_ZERO)
        .wait()
        .expect("Failed to get bonsai changeset by its globalrev counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));
}

fn bulk_import<M: BonsaiGlobalrevMapping>(mapping: M) {
    let entry1 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };
    let entry2 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_ONE,
    };
    let entry3 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::THREES_CSID,
        globalrev: GLOBALREV_TWO,
    };

    assert_eq!(
        (),
        mapping
            .bulk_import(&vec![entry1.clone(), entry2.clone(), entry3.clone()])
            .wait()
            .expect("Adding new entries vector failed")
    );
}

fn missing<M: BonsaiGlobalrevMapping>(mapping: M) {
    let result = mapping
        .get(
            REPO_ZERO,
            BonsaisOrGlobalrevs::Bonsai(vec![bonsai::ONES_CSID]),
        )
        .wait()
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert_eq!(result, vec![]);
}

#[fbinit::test]
fn test_add_and_get() {
    async_unit::tokio_unit_test(move || {
        add_and_get(SqlBonsaiGlobalrevMapping::with_sqlite_in_memory().unwrap());
    });
}

#[fbinit::test]
fn test_add_many() {
    async_unit::tokio_unit_test(move || {
        bulk_import(SqlBonsaiGlobalrevMapping::with_sqlite_in_memory().unwrap());
    });
}

#[fbinit::test]
fn test_missing() {
    async_unit::tokio_unit_test(move || {
        missing(SqlBonsaiGlobalrevMapping::with_sqlite_in_memory().unwrap());
    });
}
