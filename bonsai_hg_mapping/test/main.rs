/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Tests for the Changesets store.

#![deny(warnings)]

use anyhow::Error;
use futures::Future;

use assert_matches::assert_matches;
use bonsai_hg_mapping::{
    BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetIds, CachingBonsaiHgMapping,
    ErrorKind, MemWritesBonsaiHgMapping, SqlBonsaiHgMapping, SqlConstructors,
};
use context::CoreContext;
use fbinit::FacebookInit;
use futures_ext::BoxFuture;
use mercurial_types_mocks::nodehash as hg;
use mononoke_types::RepositoryId;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn add_and_get<M: BonsaiHgMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };
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
            .add(ctx.clone(), entry.clone())
            .wait()
            .expect("Adding same entry failed")
    );

    let result = mapping
        .get(ctx.clone(), REPO_ZERO, hg::ONES_CSID.into())
        .wait()
        .expect("Get failed");
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_hg_from_bonsai(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID)
        .wait()
        .expect("Failed to get hg changeset by its bonsai counterpart");
    assert_eq!(result, Some(hg::ONES_CSID));
    let result = mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::ONES_CSID)
        .wait()
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));

    let same_bc_entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::TWOS_CSID, // differ from entry.hg_cs_id
        bcs_id: bonsai::ONES_CSID,
    };
    let result = mapping
        .add(ctx.clone(), same_bc_entry.clone())
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
        .add(ctx.clone(), same_hg_entry.clone())
        .wait()
        .expect_err("Conflicting entries should haved produced an error");
    assert_matches!(
        result.downcast::<ErrorKind>(),
        Ok(ErrorKind::ConflictingEntries(ref e0, ref e1)) if e0 == &entry && e1 == &same_hg_entry
    );
}

fn missing<M: BonsaiHgMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let result = mapping
        .get(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID.into())
        .wait()
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert_eq!(result, vec![]);
}

fn mem_writes<M: BonsaiHgMapping + 'static>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };
    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry.clone())
            .wait()
            .expect("Adding new entry failed")
    );

    let mapping = Arc::new(mapping);
    let mem_mapping = MemWritesBonsaiHgMapping::new(mapping);

    assert_eq!(
        false,
        mem_mapping
            .add(ctx.clone(), entry.clone())
            .wait()
            .expect("Adding same entry failed")
    );

    let first_entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::TWOS_CSID,
        bcs_id: bonsai::TWOS_CSID,
    };
    assert_eq!(
        true,
        mem_mapping
            .add(ctx.clone(), first_entry.clone())
            .wait()
            .expect("Adding new entry failed")
    );

    let result = mem_mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::ONES_CSID)
        .wait()
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));

    let result = mem_mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::TWOS_CSID)
        .wait()
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::TWOS_CSID));

    let result = mem_mapping.get_ordered_inserts();
    assert_eq!(result, vec![first_entry.clone()]);

    let second_entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::THREES_CSID,
        bcs_id: bonsai::THREES_CSID,
    };
    assert_eq!(
        true,
        mem_mapping
            .add(ctx.clone(), second_entry.clone())
            .wait()
            .expect("Adding new entry failed")
    );
    let result = mem_mapping.get_ordered_inserts();
    assert_eq!(result, vec![first_entry, second_entry]);

    let inner = mem_mapping.get_inner();
    let result = inner
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::TWOS_CSID)
        .wait()
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, None);
}

struct CountedBonsaiHgMapping {
    mapping: Arc<dyn BonsaiHgMapping>,
    gets: Arc<AtomicUsize>,
    adds: Arc<AtomicUsize>,
}

impl CountedBonsaiHgMapping {
    fn new(
        mapping: Arc<dyn BonsaiHgMapping>,
        gets: Arc<AtomicUsize>,
        adds: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            mapping,
            gets,
            adds,
        }
    }
}

impl BonsaiHgMapping for CountedBonsaiHgMapping {
    fn add(&self, ctx: CoreContext, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        self.adds.fetch_add(1, Ordering::Relaxed);
        self.mapping.add(ctx, entry)
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: BonsaiOrHgChangesetIds,
    ) -> BoxFuture<Vec<BonsaiHgMappingEntry>, Error> {
        self.gets.fetch_add(1, Ordering::Relaxed);
        self.mapping.get(ctx, repo_id, cs_id)
    }
}

fn caching<M: BonsaiHgMapping + 'static>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let gets = Arc::new(AtomicUsize::new(0));
    let adds = Arc::new(AtomicUsize::new(0));
    let mapping = CountedBonsaiHgMapping::new(Arc::new(mapping), gets.clone(), adds.clone());
    let mapping = CachingBonsaiHgMapping::new_test(Arc::new(mapping));

    let entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };
    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry.clone())
            .wait()
            .expect("Adding new entry failed")
    );

    let result = mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::ONES_CSID)
        .wait()
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));
    assert_eq!(gets.load(Ordering::Relaxed), 1);

    let result = mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::ONES_CSID)
        .wait()
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));
    assert_eq!(gets.load(Ordering::Relaxed), 1);

    let result = mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::TWOS_CSID)
        .wait()
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, None);
    assert_eq!(gets.load(Ordering::Relaxed), 2);
}

#[fbinit::test]
fn test_add_and_get(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        add_and_get(fb, SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap());
    });
}

#[fbinit::test]
fn test_missing(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        missing(fb, SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap());
    });
}

#[fbinit::test]
fn test_mem_writes(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        mem_writes(fb, SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap());
    });
}

#[fbinit::test]
fn test_caching(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        caching(fb, SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap());
    });
}
