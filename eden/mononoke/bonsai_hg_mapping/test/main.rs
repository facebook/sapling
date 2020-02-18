/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the Changesets store.

#![deny(warnings)]

use anyhow::Error;

use assert_matches::assert_matches;
use bonsai_hg_mapping::{
    BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetIds, CachingBonsaiHgMapping,
    ErrorKind, SqlBonsaiHgMapping, SqlConstructors,
};
use context::CoreContext;
use fbinit::FacebookInit;
use futures_ext::BoxFuture;
use futures_preview::compat::Future01CompatExt;
use mercurial_types::{HgChangesetIdPrefix, HgChangesetIdsResolvedFromPrefix};
use mercurial_types_mocks::nodehash as hg;
use mononoke_types::RepositoryId;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;

use std::str::FromStr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

async fn add_and_get<M: BonsaiHgMapping>(fb: FacebookInit, mapping: M) {
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
            .compat()
            .await
            .expect("Adding new entry failed")
    );
    assert_eq!(
        false,
        mapping
            .add(ctx.clone(), entry.clone())
            .compat()
            .await
            .expect("Adding same entry failed")
    );

    let result = mapping
        .get(ctx.clone(), REPO_ZERO, hg::ONES_CSID.into())
        .compat()
        .await
        .expect("Get failed");
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_hg_from_bonsai(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID)
        .compat()
        .await
        .expect("Failed to get hg changeset by its bonsai counterpart");
    assert_eq!(result, Some(hg::ONES_CSID));
    let result = mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::ONES_CSID)
        .compat()
        .await
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));

    let same_bc_entry = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::TWOS_CSID, // differ from entry.hg_cs_id
        bcs_id: bonsai::ONES_CSID,
    };
    let result = mapping
        .add(ctx.clone(), same_bc_entry.clone())
        .compat()
        .await
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
        .compat()
        .await
        .expect_err("Conflicting entries should haved produced an error");
    assert_matches!(
        result.downcast::<ErrorKind>(),
        Ok(ErrorKind::ConflictingEntries(ref e0, ref e1)) if e0 == &entry && e1 == &same_hg_entry
    );
}

async fn missing<M: BonsaiHgMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let result = mapping
        .get(ctx.clone(), REPO_ZERO, bonsai::ONES_CSID.into())
        .compat()
        .await
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert_eq!(result, vec![]);
}

async fn get_many_hg_by_prefix<M: BonsaiHgMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);

    let entry1 = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };
    let entry2 = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::TWOS_CSID,
        bcs_id: bonsai::TWOS_CSID,
    };
    let entry3 = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::FS_ES_CSID,
        bcs_id: bonsai::FS_ES_CSID,
    };
    let entry4 = BonsaiHgMappingEntry {
        repo_id: REPO_ZERO,
        hg_cs_id: hg::FS_CSID,
        bcs_id: bonsai::FS_CSID,
    };

    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry1.clone())
            .compat()
            .await
            .expect("Adding entry1 failed")
    );
    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry2.clone())
            .compat()
            .await
            .expect("Adding entry2 failed")
    );
    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry3.clone())
            .compat()
            .await
            .expect("Adding entry3 failed")
    );
    assert_eq!(
        true,
        mapping
            .add(ctx.clone(), entry4.clone())
            .compat()
            .await
            .expect("Adding entry4 failed")
    );

    // found a single changeset
    let result = mapping
        .get_many_hg_by_prefix(
            ctx.clone(),
            REPO_ZERO,
            HgChangesetIdPrefix::from_bytes(&hg::ONES_CSID.as_ref()[0..8]).unwrap(),
            10,
        )
        .compat()
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::Single(hg::ONES_CSID)
    );

    // found a single changeset
    let result = mapping
        .get_many_hg_by_prefix(
            ctx.clone(),
            REPO_ZERO,
            HgChangesetIdPrefix::from_bytes(&hg::TWOS_CSID.as_ref()[0..10]).unwrap(),
            1,
        )
        .compat()
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::Single(hg::TWOS_CSID)
    );

    // found several changesets within the limit
    let result = mapping
        .get_many_hg_by_prefix(
            ctx.clone(),
            REPO_ZERO,
            HgChangesetIdPrefix::from_bytes(&hg::FS_CSID.as_ref()[0..8]).unwrap(),
            10,
        )
        .compat()
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::Multiple(vec![hg::FS_ES_CSID, hg::FS_CSID])
    );

    // found several changesets within the limit (try odd hex string prefix this time)
    let result = mapping
        .get_many_hg_by_prefix(
            ctx.clone(),
            REPO_ZERO,
            HgChangesetIdPrefix::from_str(&"fff").unwrap(),
            10,
        )
        .compat()
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::Multiple(vec![hg::FS_ES_CSID, hg::FS_CSID])
    );

    // found several changesets exceeding the limit
    let result = mapping
        .get_many_hg_by_prefix(
            ctx.clone(),
            REPO_ZERO,
            HgChangesetIdPrefix::from_bytes(&hg::FS_CSID.as_ref()[0..8]).unwrap(),
            1,
        )
        .compat()
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::TooMany(vec![hg::FS_ES_CSID])
    );

    // not found
    let result = mapping
        .get_many_hg_by_prefix(
            ctx.clone(),
            REPO_ZERO,
            HgChangesetIdPrefix::from_bytes(&hg::THREES_CSID.as_ref()[0..16]).unwrap(),
            10,
        )
        .compat()
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(result, HgChangesetIdsResolvedFromPrefix::NoMatch);
}

struct CountedBonsaiHgMapping {
    mapping: Arc<dyn BonsaiHgMapping>,
    gets: Arc<AtomicUsize>,
    adds: Arc<AtomicUsize>,
    gets_many_hg_by_prefix: Arc<AtomicUsize>,
}

impl CountedBonsaiHgMapping {
    fn new(
        mapping: Arc<dyn BonsaiHgMapping>,
        gets: Arc<AtomicUsize>,
        adds: Arc<AtomicUsize>,
        gets_many_hg_by_prefix: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            mapping,
            gets,
            adds,
            gets_many_hg_by_prefix,
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

    fn get_many_hg_by_prefix(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_prefix: HgChangesetIdPrefix,
        limit: usize,
    ) -> BoxFuture<HgChangesetIdsResolvedFromPrefix, Error> {
        self.gets_many_hg_by_prefix.fetch_add(1, Ordering::Relaxed);
        self.mapping
            .get_many_hg_by_prefix(ctx, repo_id, cs_prefix, limit)
    }
}

async fn caching<M: BonsaiHgMapping + 'static>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let gets = Arc::new(AtomicUsize::new(0));
    let adds = Arc::new(AtomicUsize::new(0));
    let gets_many_hg_by_prefix = Arc::new(AtomicUsize::new(0));
    let mapping = CountedBonsaiHgMapping::new(
        Arc::new(mapping),
        gets.clone(),
        adds.clone(),
        gets_many_hg_by_prefix.clone(),
    );
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
            .compat()
            .await
            .expect("Adding new entry failed")
    );

    let result = mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::ONES_CSID)
        .compat()
        .await
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));
    assert_eq!(gets.load(Ordering::Relaxed), 1);

    let result = mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::ONES_CSID)
        .compat()
        .await
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));
    assert_eq!(gets.load(Ordering::Relaxed), 1);

    let result = mapping
        .get_bonsai_from_hg(ctx.clone(), REPO_ZERO, hg::TWOS_CSID)
        .compat()
        .await
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, None);
    assert_eq!(gets.load(Ordering::Relaxed), 2);
}

#[fbinit::test]
fn test_add_and_get(fb: FacebookInit) {
    async_unit::tokio_unit_test(add_and_get(
        fb,
        SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap(),
    ));
}

#[fbinit::test]
fn test_missing(fb: FacebookInit) {
    async_unit::tokio_unit_test(missing(
        fb,
        SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap(),
    ));
}

#[fbinit::test]
fn test_caching(fb: FacebookInit) {
    async_unit::tokio_unit_test(caching(
        fb,
        SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap(),
    ));
}

#[fbinit::test]
fn test_get_many_hg_by_prefix(fb: FacebookInit) {
    async_unit::tokio_unit_test(get_many_hg_by_prefix(
        fb,
        SqlBonsaiHgMapping::with_sqlite_in_memory().unwrap(),
    ));
}
