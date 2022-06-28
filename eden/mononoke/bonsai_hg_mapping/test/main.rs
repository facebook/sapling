/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the Changesets store.

use anyhow::Error;
use async_trait::async_trait;

use assert_matches::assert_matches;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use bonsai_hg_mapping::BonsaiOrHgChangesetIds;
use bonsai_hg_mapping::CachingBonsaiHgMapping;
use bonsai_hg_mapping::ErrorKind;
use bonsai_hg_mapping::SqlBonsaiHgMappingBuilder;
use context::CoreContext;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use mercurial_types::HgChangesetIdPrefix;
use mercurial_types::HgChangesetIdsResolvedFromPrefix;
use mercurial_types_mocks::nodehash as hg;
use mononoke_types::RepositoryId;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;
use rendezvous::RendezVousOptions;
use sql_construct::SqlConstruct;

use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

async fn add_and_get<M: BonsaiHgMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);
    let entry = BonsaiHgMappingEntry {
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };
    assert_eq!(
        true,
        mapping
            .add(&ctx, entry.clone())
            .await
            .expect("Adding new entry failed")
    );
    assert_eq!(
        false,
        mapping
            .add(&ctx, entry.clone())
            .await
            .expect("Adding same entry failed")
    );

    let result = mapping
        .get(&ctx, hg::ONES_CSID.into())
        .await
        .expect("Get failed");
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_hg_from_bonsai(&ctx, bonsai::ONES_CSID)
        .await
        .expect("Failed to get hg changeset by its bonsai counterpart");
    assert_eq!(result, Some(hg::ONES_CSID));
    let result = mapping
        .get_bonsai_from_hg(&ctx, hg::ONES_CSID)
        .await
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));

    let same_bc_entry = BonsaiHgMappingEntry {
        hg_cs_id: hg::TWOS_CSID, // differ from entry.hg_cs_id
        bcs_id: bonsai::ONES_CSID,
    };
    let result = mapping
        .add(&ctx, same_bc_entry.clone())
        .await
        .expect_err("Conflicting entries should haved produced an error");
    assert_matches!(
        result.downcast::<ErrorKind>(),
        Ok(ErrorKind::ConflictingEntries(ref e0, ref e1)) if e0 == &entry && e1 == &same_bc_entry
    );

    let same_hg_entry = BonsaiHgMappingEntry {
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::TWOS_CSID, // differ from entry.bcs_id
    };
    let result = mapping
        .add(&ctx, same_hg_entry.clone())
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
        .get(&ctx, bonsai::ONES_CSID.into())
        .await
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert_eq!(result, vec![]);
}

async fn get_many_hg_by_prefix<M: BonsaiHgMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);

    let entry1 = BonsaiHgMappingEntry {
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };
    let entry2 = BonsaiHgMappingEntry {
        hg_cs_id: hg::TWOS_CSID,
        bcs_id: bonsai::TWOS_CSID,
    };
    let entry3 = BonsaiHgMappingEntry {
        hg_cs_id: hg::FS_ES_CSID,
        bcs_id: bonsai::FS_ES_CSID,
    };
    let entry4 = BonsaiHgMappingEntry {
        hg_cs_id: hg::FS_CSID,
        bcs_id: bonsai::FS_CSID,
    };

    assert_eq!(
        true,
        mapping
            .add(&ctx, entry1.clone())
            .await
            .expect("Adding entry1 failed")
    );
    assert_eq!(
        true,
        mapping
            .add(&ctx, entry2.clone())
            .await
            .expect("Adding entry2 failed")
    );
    assert_eq!(
        true,
        mapping
            .add(&ctx, entry3.clone())
            .await
            .expect("Adding entry3 failed")
    );
    assert_eq!(
        true,
        mapping
            .add(&ctx, entry4.clone())
            .await
            .expect("Adding entry4 failed")
    );

    // found a single changeset
    let result = mapping
        .get_many_hg_by_prefix(
            &ctx,
            HgChangesetIdPrefix::from_bytes(&hg::ONES_CSID.as_ref()[0..8]).unwrap(),
            10,
        )
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::Single(hg::ONES_CSID)
    );

    // found a single changeset
    let result = mapping
        .get_many_hg_by_prefix(
            &ctx,
            HgChangesetIdPrefix::from_bytes(&hg::TWOS_CSID.as_ref()[0..10]).unwrap(),
            1,
        )
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::Single(hg::TWOS_CSID)
    );

    // found several changesets within the limit
    let result = mapping
        .get_many_hg_by_prefix(
            &ctx,
            HgChangesetIdPrefix::from_bytes(&hg::FS_CSID.as_ref()[0..8]).unwrap(),
            10,
        )
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::Multiple(vec![hg::FS_ES_CSID, hg::FS_CSID])
    );

    // found several changesets within the limit (try odd hex string prefix this time)
    let result = mapping
        .get_many_hg_by_prefix(&ctx, HgChangesetIdPrefix::from_str("fff").unwrap(), 10)
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::Multiple(vec![hg::FS_ES_CSID, hg::FS_CSID])
    );

    // found several changesets exceeding the limit
    let result = mapping
        .get_many_hg_by_prefix(
            &ctx,
            HgChangesetIdPrefix::from_bytes(&hg::FS_CSID.as_ref()[0..8]).unwrap(),
            1,
        )
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(
        result,
        HgChangesetIdsResolvedFromPrefix::TooMany(vec![hg::FS_ES_CSID])
    );

    // not found
    let result = mapping
        .get_many_hg_by_prefix(
            &ctx,
            HgChangesetIdPrefix::from_bytes(&hg::THREES_CSID.as_ref()[0..16]).unwrap(),
            10,
        )
        .await
        .expect("Failed to get hg changeset by its prefix");

    assert_eq!(result, HgChangesetIdsResolvedFromPrefix::NoMatch);
}

async fn get_hg_in_range<M: BonsaiHgMapping>(fb: FacebookInit, mapping: M) {
    let ctx = CoreContext::test_mock(fb);

    let entry1 = BonsaiHgMappingEntry {
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };
    let entry2 = BonsaiHgMappingEntry {
        hg_cs_id: hg::TWOS_CSID,
        bcs_id: bonsai::TWOS_CSID,
    };
    let entry3 = BonsaiHgMappingEntry {
        hg_cs_id: hg::THREES_CSID,
        bcs_id: bonsai::THREES_CSID,
    };

    for entry in [entry1, entry2, entry3] {
        assert_eq!(true, mapping.add(&ctx, entry).await.unwrap());
    }

    assert!(
        mapping
            .get_hg_in_range(&ctx, hg::AS_CSID, hg::BS_CSID, 10)
            .await
            .unwrap()
            .is_empty(),
    );

    assert_eq!(
        vec![hg::ONES_CSID],
        mapping
            .get_hg_in_range(&ctx, hg::ONES_CSID, hg::ONES_CSID, 10)
            .await
            .unwrap()
    );

    assert_eq!(
        vec![hg::ONES_CSID, hg::TWOS_CSID],
        mapping
            .get_hg_in_range(&ctx, hg::ONES_CSID, hg::TWOS_CSID, 10)
            .await
            .unwrap()
    );

    assert_eq!(
        vec![hg::ONES_CSID],
        mapping
            .get_hg_in_range(&ctx, hg::ONES_CSID, hg::TWOS_CSID, 1)
            .await
            .unwrap()
    );

    assert_eq!(
        vec![hg::ONES_CSID, hg::TWOS_CSID, hg::THREES_CSID],
        mapping
            .get_hg_in_range(&ctx, hg::NULL_CSID, hg::FS_CSID, 10)
            .await
            .unwrap()
    );
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

#[async_trait]
impl BonsaiHgMapping for CountedBonsaiHgMapping {
    fn repo_id(&self) -> RepositoryId {
        self.mapping.repo_id()
    }

    async fn add(&self, ctx: &CoreContext, entry: BonsaiHgMappingEntry) -> Result<bool, Error> {
        self.adds.fetch_add(1, Ordering::Relaxed);
        self.mapping.add(ctx, entry).await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        cs_id: BonsaiOrHgChangesetIds,
    ) -> Result<Vec<BonsaiHgMappingEntry>, Error> {
        self.gets.fetch_add(1, Ordering::Relaxed);
        self.mapping.get(ctx, cs_id).await
    }

    async fn get_hg_in_range(
        &self,
        ctx: &CoreContext,
        low: HgChangesetId,
        high: HgChangesetId,
        limit: usize,
    ) -> Result<Vec<HgChangesetId>, Error> {
        self.gets_many_hg_by_prefix.fetch_add(1, Ordering::Relaxed);
        self.mapping.get_hg_in_range(ctx, low, high, limit).await
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
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };
    assert_eq!(
        true,
        mapping
            .add(&ctx, entry.clone())
            .await
            .expect("Adding new entry failed")
    );

    let result = mapping
        .get_bonsai_from_hg(&ctx, hg::ONES_CSID)
        .await
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));
    assert_eq!(gets.load(Ordering::Relaxed), 1);

    let result = mapping
        .get_bonsai_from_hg(&ctx, hg::ONES_CSID)
        .await
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, Some(bonsai::ONES_CSID));
    assert_eq!(gets.load(Ordering::Relaxed), 1);

    let result = mapping
        .get_bonsai_from_hg(&ctx, hg::TWOS_CSID)
        .await
        .expect("Failed to get bonsai changeset by its hg counterpart");
    assert_eq!(result, None);
    assert_eq!(gets.load(Ordering::Relaxed), 2);
}

#[fbinit::test]
async fn test_add_and_get(fb: FacebookInit) {
    add_and_get(
        fb,
        SqlBonsaiHgMappingBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(REPO_ZERO, RendezVousOptions::for_test()),
    )
    .await;
}

#[fbinit::test]
async fn test_missing(fb: FacebookInit) {
    missing(
        fb,
        SqlBonsaiHgMappingBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(REPO_ZERO, RendezVousOptions::for_test()),
    )
    .await;
}

#[fbinit::test]
async fn test_caching(fb: FacebookInit) {
    caching(
        fb,
        SqlBonsaiHgMappingBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(REPO_ZERO, RendezVousOptions::for_test()),
    )
    .await;
}

#[fbinit::test]
async fn test_get_many_hg_by_prefix(fb: FacebookInit) {
    get_many_hg_by_prefix(
        fb,
        SqlBonsaiHgMappingBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(REPO_ZERO, RendezVousOptions::for_test()),
    )
    .await;
}

#[fbinit::test]
async fn test_get_hg_in_range(fb: FacebookInit) {
    get_hg_in_range(
        fb,
        SqlBonsaiHgMappingBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(REPO_ZERO, RendezVousOptions::for_test()),
    )
    .await;
}

#[fbinit::test]
async fn test_overwrite(fb: FacebookInit) -> Result<(), Error> {
    let mapping = SqlBonsaiHgMappingBuilder::with_sqlite_in_memory()
        .unwrap()
        .with_overwrite()
        .build(REPO_ZERO, RendezVousOptions::for_test());

    let ctx = CoreContext::test_mock(fb);
    let entry = BonsaiHgMappingEntry {
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::ONES_CSID,
    };

    assert_eq!(
        true,
        mapping
            .add(&ctx, entry.clone())
            .await
            .expect("Adding new entry failed")
    );

    let entry = BonsaiHgMappingEntry {
        hg_cs_id: hg::ONES_CSID,
        bcs_id: bonsai::TWOS_CSID,
    };
    assert_eq!(
        true,
        mapping
            .add(&ctx, entry.clone())
            .await
            .expect("Adding new entry failed")
    );

    let result = mapping.get(&ctx, hg::ONES_CSID.into()).await?;
    assert_eq!(result, vec![entry.clone()]);

    Ok(())
}
