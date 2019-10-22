/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use assert_matches::assert_matches;
use bonsai_globalrev_mapping::{
    BonsaiGlobalrevMapping, BonsaiGlobalrevMappingEntry, BonsaisOrGlobalrevs, ErrorKind,
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

    let same_bc_entry = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ONE, // different than entry.globalrev
    };
    let result = mapping
        .add(same_bc_entry.clone())
        .wait()
        .expect_err("Conflicting entries should haved produced an error");
    assert_matches!(
        result.downcast::<ErrorKind>(),
        Ok(ErrorKind::ConflictingEntries(ref e0, ref e1)) if e0 == &entry && e1 == &same_bc_entry
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
fn test_missing() {
    async_unit::tokio_unit_test(move || {
        missing(SqlBonsaiGlobalrevMapping::with_sqlite_in_memory().unwrap());
    });
}
