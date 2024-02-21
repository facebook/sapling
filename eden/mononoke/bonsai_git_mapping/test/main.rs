/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use ::sql::Transaction;
use anyhow::Error;
use assert_matches::assert_matches;
use async_trait::async_trait;
use bonsai_git_mapping::AddGitMappingErrorKind;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaisOrGitShas;
use bonsai_git_mapping::CachingBonsaiGitMapping;
use bonsai_git_mapping::SqlBonsaiGitMappingBuilder;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types::hash::GitSha1;
use mononoke_types::RepositoryId;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::hash::*;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_ext::open_sqlite_in_memory;
use sql_ext::SqlConnections;

struct CountedBonsaiGitMapping {
    inner_mapping: Arc<dyn BonsaiGitMapping>,
    fetched_entries: Arc<AtomicUsize>,
}

impl CountedBonsaiGitMapping {
    pub fn new(inner_mapping: Arc<dyn BonsaiGitMapping>) -> Self {
        Self {
            inner_mapping,
            fetched_entries: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn fetched_entries_counter(&self) -> Arc<AtomicUsize> {
        self.fetched_entries.clone()
    }
}

#[async_trait]
impl BonsaiGitMapping for CountedBonsaiGitMapping {
    fn repo_id(&self) -> RepositoryId {
        self.inner_mapping.repo_id()
    }

    async fn add(
        &self,
        ctx: &CoreContext,
        entry: BonsaiGitMappingEntry,
    ) -> Result<(), AddGitMappingErrorKind> {
        self.inner_mapping.add(ctx, entry).await
    }

    async fn bulk_add(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind> {
        self.inner_mapping.bulk_add(ctx, entries).await
    }

    async fn bulk_add_git_mapping_in_transaction(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
        transaction: Transaction,
    ) -> Result<Transaction, AddGitMappingErrorKind> {
        self.inner_mapping
            .bulk_add_git_mapping_in_transaction(ctx, entries, transaction)
            .await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        cs: BonsaisOrGitShas,
    ) -> Result<Vec<BonsaiGitMappingEntry>, Error> {
        self.fetched_entries
            .fetch_add(cs.count(), Ordering::Relaxed);
        self.inner_mapping.get(ctx, cs).await
    }

    /// Use caching for the ranges of one element, use slower path otherwise.
    async fn get_in_range(
        &self,
        ctx: &CoreContext,
        low: GitSha1,
        high: GitSha1,
        limit: usize,
    ) -> Result<Vec<GitSha1>, Error> {
        self.inner_mapping.get_in_range(ctx, low, high, limit).await
    }
}

#[fbinit::test]
async fn test_add_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGitMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let entry = BonsaiGitMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };

    mapping.bulk_add(&ctx, &[entry.clone()]).await?;

    let result = mapping
        .get(&ctx, BonsaisOrGitShas::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_git_sha1_from_bonsai(&ctx, bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, Some(ONES_GIT_SHA1));
    let result = mapping
        .get_bonsai_from_git_sha1(&ctx, ONES_GIT_SHA1)
        .await?;
    assert_eq!(result, Some(bonsai::ONES_CSID));

    Ok(())
}

#[fbinit::test]
async fn test_add_duplicate(fb: FacebookInit) -> Result<(), Error> {
    // Inserting duplicate entries should just be a successful no-op.
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGitMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let entry = BonsaiGitMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };

    mapping.bulk_add(&ctx, &[entry.clone()]).await?;
    mapping.bulk_add(&ctx, &[entry.clone()]).await?;

    let result = mapping
        .get_git_sha1_from_bonsai(&ctx, bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, Some(ONES_GIT_SHA1));

    Ok(())
}

#[fbinit::test]
async fn test_add_conflict(fb: FacebookInit) -> Result<(), Error> {
    // Adding conflicting entries should fail. Other entries inserted in the
    // same bulk_add should be inserted.
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGitMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let entry = BonsaiGitMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };

    mapping.bulk_add(&ctx, &[entry.clone()]).await?;

    let entries = vec![
        BonsaiGitMappingEntry {
            // This entry could be inserted normally, but it won't because we have a conflicting
            // entry
            bcs_id: bonsai::TWOS_CSID,
            git_sha1: TWOS_GIT_SHA1,
        },
        BonsaiGitMappingEntry {
            // Conflicting entry.
            bcs_id: bonsai::ONES_CSID,
            git_sha1: THREES_GIT_SHA1,
        },
    ];

    let res = mapping.bulk_add(&ctx, &entries).await;
    assert_matches!(
        res,
        Err(AddGitMappingErrorKind::Conflict(
            Some(BonsaiGitMappingEntry {
                bcs_id: bonsai::ONES_CSID,
                git_sha1: ONES_GIT_SHA1,
            }),
            _
        ))
    );

    let result = mapping
        .get_git_sha1_from_bonsai(&ctx, bonsai::TWOS_CSID)
        .await?;
    assert_eq!(result, None);

    let entries = vec![BonsaiGitMappingEntry {
        // Now this entry will be inserted normally
        bcs_id: bonsai::TWOS_CSID,
        git_sha1: TWOS_GIT_SHA1,
    }];

    mapping.bulk_add(&ctx, &entries).await?;
    let result = mapping
        .get_git_sha1_from_bonsai(&ctx, bonsai::TWOS_CSID)
        .await?;
    assert_eq!(result, Some(TWOS_GIT_SHA1));

    Ok(())
}

#[fbinit::test]
async fn test_bulk_add(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGitMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let entry1 = BonsaiGitMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };
    let entry2 = BonsaiGitMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        git_sha1: TWOS_GIT_SHA1,
    };
    let entry3 = BonsaiGitMappingEntry {
        bcs_id: bonsai::THREES_CSID,
        git_sha1: THREES_GIT_SHA1,
    };

    mapping
        .bulk_add(&ctx, &[entry1.clone(), entry2.clone(), entry3.clone()])
        .await?;

    Ok(())
}

#[fbinit::test]
async fn test_missing(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGitMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let result = mapping
        .get(&ctx, BonsaisOrGitShas::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;

    assert_eq!(result, vec![]);

    Ok(())
}

#[fbinit::test]
async fn test_add_with_transaction(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let conn = open_sqlite_in_memory()?;
    conn.execute_batch(SqlBonsaiGitMappingBuilder::CREATION_QUERY)?;
    let conn = Connection::with_sqlite(conn);

    let mapping =
        SqlBonsaiGitMappingBuilder::from_sql_connections(SqlConnections::new_single(conn.clone()))
            .build(REPO_ZERO);

    let entry1 = BonsaiGitMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };
    let entry2 = BonsaiGitMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        git_sha1: TWOS_GIT_SHA1,
    };

    let txn = conn.start_transaction().await?;
    mapping
        .bulk_add_git_mapping_in_transaction(&ctx, &[entry1.clone()], txn)
        .await?
        .commit()
        .await?;

    assert_eq!(
        Some(ONES_GIT_SHA1),
        mapping
            .get_git_sha1_from_bonsai(&ctx, bonsai::ONES_CSID)
            .await?
    );

    let txn = conn.start_transaction().await?;
    mapping
        .bulk_add_git_mapping_in_transaction(&ctx, &[entry2.clone()], txn)
        .await?
        .commit()
        .await?;

    assert_eq!(
        Some(TWOS_GIT_SHA1),
        mapping
            .get_git_sha1_from_bonsai(&ctx, bonsai::TWOS_CSID)
            .await?
    );

    // Inserting duplicates fails
    let txn = conn.start_transaction().await?;
    let res = async {
        let entry_conflict = BonsaiGitMappingEntry {
            bcs_id: bonsai::TWOS_CSID,
            git_sha1: THREES_GIT_SHA1,
        };
        mapping
            .bulk_add_git_mapping_in_transaction(&ctx, &[entry_conflict], txn)
            .await?
            .commit()
            .await?;
        Result::<_, AddGitMappingErrorKind>::Ok(())
    }
    .await;
    assert_matches!(
        res,
        Err(AddGitMappingErrorKind::Conflict(
            Some(BonsaiGitMappingEntry {
                bcs_id: bonsai::TWOS_CSID,
                git_sha1: TWOS_GIT_SHA1,
            }),
            _
        ))
    );

    assert_eq!(
        Some(TWOS_GIT_SHA1),
        mapping
            .get_git_sha1_from_bonsai(&ctx, bonsai::TWOS_CSID)
            .await?
    );

    Ok(())
}

#[fbinit::test]
async fn test_get_with_caching(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let conn = open_sqlite_in_memory()?;
    conn.execute_batch(SqlBonsaiGitMappingBuilder::CREATION_QUERY)?;
    let conn = Connection::with_sqlite(conn);
    let mapping =
        SqlBonsaiGitMappingBuilder::from_sql_connections(SqlConnections::new_single(conn.clone()))
            .build(REPO_ZERO);
    let counted_mapping = CountedBonsaiGitMapping::new(Arc::new(mapping));
    let fetched_entries = counted_mapping.fetched_entries_counter();
    let caching_mapping = CachingBonsaiGitMapping::new_test(Arc::new(counted_mapping));

    // Populate a few bonsai_git_mappings
    let entry1 = BonsaiGitMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };
    let entry2 = BonsaiGitMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        git_sha1: TWOS_GIT_SHA1,
    };
    let entry3 = BonsaiGitMappingEntry {
        bcs_id: bonsai::THREES_CSID,
        git_sha1: THREES_GIT_SHA1,
    };

    caching_mapping
        .bulk_add(&ctx, &[entry1.clone(), entry2.clone(), entry3.clone()])
        .await?;

    // Fetch a single bonsai_git_mapping. Since this is the first time we are fetching it,
    // we expect that the counter has been incremented by 1.
    assert_eq!(fetched_entries.load(Ordering::Relaxed), 0);
    let result = caching_mapping
        .get(&ctx, BonsaisOrGitShas::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;
    assert_eq!(fetched_entries.load(Ordering::Relaxed), 1);
    assert_eq!(result, vec![entry1.clone()]);

    // Fetch the same bonsai_git_mapping again. This time, since we have already fetched it once,
    // we expect that the counter has not changed.
    let result = caching_mapping
        .get(&ctx, BonsaisOrGitShas::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;
    assert_eq!(fetched_entries.load(Ordering::Relaxed), 1);
    assert_eq!(result, vec![entry1]);

    // Fetch multiple bonsai_git_mappings at once. The counter should be incremented for the entries that
    // we haven't fetched so far
    let result = caching_mapping
        .get(
            &ctx,
            BonsaisOrGitShas::Bonsai(vec![
                bonsai::ONES_CSID,
                bonsai::TWOS_CSID,
                bonsai::THREES_CSID,
            ]),
        )
        .await?;
    assert_eq!(fetched_entries.load(Ordering::Relaxed), 3);
    assert!(result.len() == 3);
    Ok(())
}
