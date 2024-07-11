/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the Changesets store.
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Error;
use assert_matches::assert_matches;
use caching_ext::MockStoreStats;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets::ChangesetsRef;
use changesets::SortOrder;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::Future;
use futures::TryStreamExt;
use maplit::hashset;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types_mocks::changesetid::*;
use mononoke_types_mocks::repo::*;
use pretty_assertions::assert_eq;
use rendezvous::RendezVousOptions;
use sql_construct::SqlConstruct;
use vec1::Vec1;

use super::CachingChangesets;
use super::SqlChangesets;
use super::SqlChangesetsBuilder;
use crate::sql::SqlChangesetsError;

async fn run_test<F, FO>(fb: FacebookInit, test_fn: F) -> Result<(), Error>
where
    F: FnOnce(FacebookInit, SqlChangesets) -> FO,
    FO: Future<Output = Result<(), Error>>,
{
    test_fn(
        fb,
        SqlChangesetsBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), REPO_ZERO),
    )
    .await?;
    Ok(())
}

async fn run_caching_test<F, FO>(fb: FacebookInit, test_fn: F) -> Result<(), Error>
where
    F: FnOnce(FacebookInit, CachingChangesets) -> FO,
    FO: Future<Output = Result<(), Error>>,
{
    let real_changesets = Arc::new(
        SqlChangesetsBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), REPO_ZERO),
    );
    let changesets = CachingChangesets::mocked(real_changesets);
    test_fn(fb, changesets).await?;
    Ok(())
}

async fn add_and_get<C: Changesets + 'static>(
    fb: FacebookInit,
    changesets: C,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;

    let row = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };

    changesets.add(ctx, row).await?;
    let result = changesets.get(ctx, ONES_CSID).await?;
    assert_eq!(
        result,
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: ONES_CSID,
            parents: vec![],
            gen: 1,
        }),
    );
    Ok(())
}

async fn add_missing_parents<C: Changesets>(fb: FacebookInit, changesets: C) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;

    let row = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![TWOS_CSID],
    };

    let result = changesets
        .add(ctx, row)
        .await
        .expect_err("Adding entry with missing parents failed (should have succeeded)");
    assert_matches!(
        result.downcast::<SqlChangesetsError>(),
        Ok(SqlChangesetsError::MissingParents(ref x)) if x == &vec![TWOS_CSID]
    );
    Ok(())
}

async fn missing<C: Changesets + 'static>(fb: FacebookInit, changesets: C) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;
    let result = changesets
        .get(ctx, ONES_CSID)
        .await
        .expect("Failed to fetch missing changeset (should succeed with None instead)");
    assert_eq!(result, None);
    Ok(())
}

async fn duplicate<C: Changesets + 'static>(fb: FacebookInit, changesets: C) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;
    let row = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };

    assert!(
        changesets.add(ctx, row.clone()).await?,
        "inserting unique changeset must return true"
    );

    assert!(
        !changesets.add(ctx, row.clone()).await?,
        "inserting the same changeset must return false"
    );
    Ok(())
}

async fn broken_duplicate<C: Changesets + 'static>(
    fb: FacebookInit,
    changesets: C,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;
    let row = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    assert!(
        changesets.add(ctx, row).await?,
        "inserting unique changeset must return true"
    );

    let row = ChangesetInsert {
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    assert!(
        changesets.add(ctx, row).await?,
        "inserting unique changeset must return true"
    );

    let row = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![TWOS_CSID],
    };
    let result = changesets
        .add(ctx, row)
        .await
        .expect_err("Adding changeset with the same hash but differen parents should fail");
    match result.downcast::<SqlChangesetsError>() {
        Ok(SqlChangesetsError::DuplicateInsertionInconsistency(..)) => {}
        err => panic!("unexpected error: {:?}", err),
    };

    Ok(())
}

async fn complex<C: Changesets>(fb: FacebookInit, changesets: C) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;

    let row1 = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    changesets.add(ctx, row1).await?;

    let row2 = ChangesetInsert {
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    changesets.add(ctx, row2).await?;

    let row3 = ChangesetInsert {
        cs_id: THREES_CSID,
        parents: vec![TWOS_CSID],
    };
    changesets.add(ctx, row3).await?;

    let row4 = ChangesetInsert {
        cs_id: FOURS_CSID,
        parents: vec![ONES_CSID, THREES_CSID],
    };
    changesets.add(ctx, row4).await?;

    let row5 = ChangesetInsert {
        cs_id: FIVES_CSID,
        parents: vec![ONES_CSID, TWOS_CSID, FOURS_CSID],
    };
    changesets.add(ctx, row5).await?;

    assert_eq!(
        changesets.get(ctx, ONES_CSID).await?,
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: ONES_CSID,
            parents: vec![],
            gen: 1,
        }),
    );

    assert_eq!(
        changesets.get(ctx, TWOS_CSID).await?,
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: TWOS_CSID,
            parents: vec![],
            gen: 1,
        }),
    );

    assert_eq!(
        changesets.get(ctx, THREES_CSID).await?,
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: THREES_CSID,
            parents: vec![TWOS_CSID],
            gen: 2,
        }),
    );

    assert_eq!(
        changesets.get(ctx, FOURS_CSID).await?,
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: FOURS_CSID,
            parents: vec![ONES_CSID, THREES_CSID],
            gen: 3,
        }),
    );

    assert_eq!(
        changesets.get(ctx, FIVES_CSID).await?,
        Some(ChangesetEntry {
            repo_id: REPO_ZERO,
            cs_id: FIVES_CSID,
            parents: vec![ONES_CSID, TWOS_CSID, FOURS_CSID],
            gen: 4,
        }),
    );

    Ok(())
}

async fn get_many<C: Changesets>(fb: FacebookInit, changesets: C) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;

    let row1 = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    changesets.add(ctx, row1).await?;

    let row2 = ChangesetInsert {
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    changesets.add(ctx, row2).await?;

    let row3 = ChangesetInsert {
        cs_id: THREES_CSID,
        parents: vec![TWOS_CSID],
    };
    changesets.add(ctx, row3).await?;

    let row4 = ChangesetInsert {
        cs_id: FOURS_CSID,
        parents: vec![ONES_CSID, THREES_CSID],
    };
    changesets.add(ctx, row4).await?;

    let row5 = ChangesetInsert {
        cs_id: FIVES_CSID,
        parents: vec![THREES_CSID, ONES_CSID, TWOS_CSID, FOURS_CSID],
    };
    changesets.add(ctx, row5).await?;

    let actual = changesets.get_many(ctx, vec![ONES_CSID, TWOS_CSID]).await?;

    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: TWOS_CSID,
                parents: vec![],
                gen: 1,
            },
        ]
    );

    let actual = changesets
        .get_many(ctx, vec![ONES_CSID, TWOS_CSID, THREES_CSID])
        .await?;
    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: TWOS_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: THREES_CSID,
                parents: vec![TWOS_CSID],
                gen: 2,
            },
        ]
    );

    let actual = changesets.get_many(ctx, vec![]).await?;
    assert_eq!(HashSet::from_iter(actual), hashset![]);

    let actual = changesets
        .get_many(ctx, vec![ONES_CSID, FOURS_CSID])
        .await?;
    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: FOURS_CSID,
                parents: vec![ONES_CSID, THREES_CSID],
                gen: 3,
            },
        ]
    );

    let actual = changesets
        .get_many(ctx, vec![ONES_CSID, FOURS_CSID, FIVES_CSID])
        .await?;
    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: FOURS_CSID,
                parents: vec![ONES_CSID, THREES_CSID],
                gen: 3,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: FIVES_CSID,
                parents: vec![THREES_CSID, ONES_CSID, TWOS_CSID, FOURS_CSID],
                gen: 4,
            },
        ]
    );

    Ok(())
}

async fn get_many_missing<C: Changesets>(fb: FacebookInit, changesets: C) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;

    let row1 = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    changesets.add(ctx, row1).await?;

    let row2 = ChangesetInsert {
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    changesets.add(ctx, row2).await?;

    let actual = changesets
        .get_many(ctx, vec![ONES_CSID, TWOS_CSID, THREES_CSID])
        .await?;
    assert_eq!(
        HashSet::from_iter(actual),
        hashset![
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: ONES_CSID,
                parents: vec![],
                gen: 1,
            },
            ChangesetEntry {
                repo_id: REPO_ZERO,
                cs_id: TWOS_CSID,
                parents: vec![],
                gen: 1,
            },
        ]
    );

    Ok(())
}

async fn get_many_by_prefix<C: Changesets>(fb: FacebookInit, changesets: C) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;

    let row1 = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    let row2 = ChangesetInsert {
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    let row3 = ChangesetInsert {
        cs_id: FS_ES_CSID,
        parents: vec![],
    };
    let row4 = ChangesetInsert {
        cs_id: FS_CSID,
        parents: vec![],
    };

    changesets.add(ctx, row1).await?;
    changesets.add(ctx, row2).await?;
    changesets.add(ctx, row3).await?;
    changesets.add(ctx, row4).await?;

    // found a single changeset
    let actual = changesets
        .get_many_by_prefix(
            ctx,
            ChangesetIdPrefix::from_bytes(&ONES_CSID.as_ref()[0..8]).unwrap(),
            10,
        )
        .await?;
    assert_eq!(actual, ChangesetIdsResolvedFromPrefix::Single(ONES_CSID));

    // found a single changeset
    let actual = changesets
        .get_many_by_prefix(
            ctx,
            ChangesetIdPrefix::from_bytes(&TWOS_CSID.as_ref()[0..12]).unwrap(),
            1,
        )
        .await?;
    assert_eq!(actual, ChangesetIdsResolvedFromPrefix::Single(TWOS_CSID));

    // found several changesets within the limit
    let actual = changesets
        .get_many_by_prefix(
            ctx,
            ChangesetIdPrefix::from_bytes(&FS_CSID.as_ref()[0..8]).unwrap(),
            10,
        )
        .await?;
    assert_eq!(
        actual,
        ChangesetIdsResolvedFromPrefix::Multiple(vec![FS_ES_CSID, FS_CSID]),
    );

    // found several changesets within the limit by hex string prefix
    let actual = changesets
        .get_many_by_prefix(ctx, ChangesetIdPrefix::from_str("fff").unwrap(), 10)
        .await?;
    assert_eq!(
        actual,
        ChangesetIdsResolvedFromPrefix::Multiple(vec![FS_ES_CSID, FS_CSID]),
    );

    // found several changesets exceeding the limit
    let actual = changesets
        .get_many_by_prefix(
            ctx,
            ChangesetIdPrefix::from_bytes(&FS_CSID.as_ref()[0..8]).unwrap(),
            1,
        )
        .await?;
    assert_eq!(
        actual,
        ChangesetIdsResolvedFromPrefix::TooMany(vec![FS_ES_CSID]),
    );

    // not found
    let actual = changesets
        .get_many_by_prefix(
            ctx,
            ChangesetIdPrefix::from_bytes(&THREES_CSID.as_ref()[0..16]).unwrap(),
            10,
        )
        .await?;
    assert_eq!(actual, ChangesetIdsResolvedFromPrefix::NoMatch);

    Ok(())
}

async fn caching_fill<C: Changesets + 'static>(
    fb: FacebookInit,
    changesets: C,
) -> Result<(), Error> {
    let changesets = Arc::new(changesets);
    let mut cc = CachingChangesets::mocked(changesets.clone());
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;

    let row1 = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    let row2 = ChangesetInsert {
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    let row3 = ChangesetInsert {
        cs_id: THREES_CSID,
        parents: vec![],
    };

    changesets.add(ctx, row1).await?;
    changesets.add(ctx, row2).await?;
    changesets.add(ctx, row3).await?;

    // First read should miss everyhwere.
    let _ = cc.get_many(ctx, vec![ONES_CSID, TWOS_CSID]).await?;
    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 2,
            sets: 2,
            misses: 2,
            hits: 0
        },
        "read 1, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 2,
            sets: 2,
            misses: 2,
            hits: 0
        },
        "read 1, memcache"
    );

    // Another read with the same pool should hit in local cache.
    let _ = cc.get_many(ctx, vec![ONES_CSID, TWOS_CSID]).await?;
    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 4,
            sets: 2,
            misses: 2,
            hits: 2
        },
        "read 2, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 2,
            sets: 2,
            misses: 2,
            hits: 0
        },
        "read 2, memcache"
    );

    cc = cc.fork_cachelib();

    // Read with a separate pool should hit in Memcache.
    let _ = cc.get_many(ctx, vec![ONES_CSID, TWOS_CSID]).await?;
    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 2,
            sets: 2,
            misses: 2,
            hits: 0
        },
        "read 3, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 4,
            sets: 2,
            misses: 2,
            hits: 2
        },
        "read 3, memcache"
    );

    // Reading again from the separate pool should now hit in local cache.
    let _ = cc.get_many(ctx, vec![ONES_CSID, TWOS_CSID]).await?;
    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 4,
            sets: 2,
            misses: 2,
            hits: 2
        },
        "read 4, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 4,
            sets: 2,
            misses: 2,
            hits: 2
        },
        "read 4, memcache"
    );

    // Partial read should partially hit
    let _ = cc
        .get_many(ctx, vec![ONES_CSID, TWOS_CSID, THREES_CSID])
        .await?;
    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 7,
            sets: 3,
            misses: 3,
            hits: 4
        },
        "read 5, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 5,
            sets: 3,
            misses: 3,
            hits: 2
        },
        "read 5, memcache"
    );

    // Partial read should have filled local cache.
    let _ = cc
        .get_many(ctx, vec![ONES_CSID, TWOS_CSID, THREES_CSID])
        .await?;
    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 10,
            sets: 3,
            misses: 3,
            hits: 7
        },
        "read 6, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 5,
            sets: 3,
            misses: 3,
            hits: 2
        },
        "read 6, memcache"
    );

    Ok(())
}

async fn caching_shared<C: Changesets + 'static>(
    fb: FacebookInit,
    changesets: C,
) -> Result<(), Error> {
    let changesets = Arc::new(changesets);
    let cc = CachingChangesets::mocked(changesets.clone());
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;

    let row1 = ChangesetInsert {
        cs_id: ONES_CSID,
        parents: vec![],
    };
    let row2 = ChangesetInsert {
        cs_id: TWOS_CSID,
        parents: vec![],
    };
    let row3 = ChangesetInsert {
        cs_id: THREES_CSID,
        parents: vec![],
    };

    changesets.add(ctx, row1).await?;
    changesets.add(ctx, row2).await?;
    changesets.add(ctx, row3).await?;

    // get should miss
    let _ = cc.get(ctx, ONES_CSID).await?;
    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 1,
            sets: 1,
            misses: 1,
            hits: 0
        },
        "read 1, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 1,
            sets: 1,
            misses: 1,
            hits: 0
        },
        "read 1, memcache"
    );

    // get_many should hit for what was filled by get
    let _ = cc
        .get_many(ctx, vec![ONES_CSID, TWOS_CSID, THREES_CSID])
        .await?;
    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 4,
            sets: 3,
            misses: 3,
            hits: 1
        },
        "read 2, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 3,
            sets: 3,
            misses: 3,
            hits: 0
        },
        "read 2, memcache"
    );

    // get should hit for what was filled by get_many
    let _ = cc.get(ctx, THREES_CSID).await?;
    assert_eq!(
        cc.cachelib_stats(),
        MockStoreStats {
            gets: 5,
            sets: 3,
            misses: 3,
            hits: 2
        },
        "read 3, cachelib"
    );
    assert_eq!(
        cc.memcache_stats(),
        MockStoreStats {
            gets: 3,
            sets: 3,
            misses: 3,
            hits: 0
        },
        "read 3, memcache"
    );

    Ok(())
}

async fn test_add_many_fixture<F: fixtures::TestRepoFixture + Send, C: Changesets>(
    fb: FacebookInit,
    changesets: &C,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo = F::get_test_repo(fb).await;
    let all_changeset_ids: Vec<_> = repo
        .changesets()
        .list_enumeration_range(&ctx, 0, 10000, Some((SortOrder::Ascending, 10000)), false)
        .map_ok(|(cs_id, _)| cs_id)
        .try_collect()
        .await?;
    let mut all_changesets = repo
        .changesets()
        .get_many(&ctx, all_changeset_ids.clone())
        .await?;
    all_changesets.sort_by_key(|entry| (entry.gen, entry.cs_id));
    {
        // Let's insert in two parts to make sure it works fine with
        // pre-existing parents
        let insert_data: Vec<_> = all_changesets
            .clone()
            .into_iter()
            .map(|entry| ChangesetInsert {
                cs_id: entry.cs_id,
                parents: entry.parents,
            })
            .collect();
        let mid = insert_data.len() / 2;
        let left_part = &insert_data[..mid];
        // Have some overlap to test we can reinsert changesets without issue
        let right_part = &insert_data[(mid.max(2) - 2)..];
        if let Ok(data) = Vec1::try_from(left_part) {
            changesets.add_many(&ctx, data).await?;
        }
        if let Ok(data) = Vec1::try_from(right_part) {
            changesets.add_many(&ctx, data).await?;
        }
    }
    let mut new_changesets = changesets.get_many(&ctx, all_changeset_ids).await?;
    new_changesets.sort_by_key(|entry| (entry.gen, entry.cs_id));
    assert_eq!(all_changesets, new_changesets);

    Ok(())
}

// NOTE: Use this wrapper macro to make sure tests are executed both with Changesets and
// CachingChangesets. Define tests using #[test] if you need to only execute them for Changesets or
// CachingChangesets.
macro_rules! testify {
    ($test: ident) => {
        paste::item! {
            #[fbinit::test]
            async fn [<test_ $test>](fb: FacebookInit) -> Result<(), Error> {
                run_test(fb, $test).await
            }

            #[fbinit::test]
            async fn [<test_caching_ $test>](fb: FacebookInit) -> Result<(), Error> {
                run_caching_test(fb, $test).await
            }
        }
    };
}

testify!(add_and_get);
testify!(add_missing_parents);
testify!(missing);
testify!(duplicate);
testify!(broken_duplicate);
testify!(complex);
testify!(get_many);
testify!(get_many_by_prefix);
testify!(get_many_missing);

#[fbinit::test]
async fn test_caching_fill(fb: FacebookInit) -> Result<(), Error> {
    run_test(fb, caching_fill).await
}

#[fbinit::test]
async fn test_caching_shared(fb: FacebookInit) -> Result<(), Error> {
    run_test(fb, caching_shared).await
}

macro_rules! add_many_tests {
    ($fixture: ident) => {
        paste::item! {
            async fn [<add_many_ $fixture:snake>]<C: Changesets>(fb: FacebookInit, changesets: C) -> Result<(), Error> {
                test_add_many_fixture::<fixtures::$fixture, C>(fb, &changesets).await
            }
            testify!([<add_many_ $fixture:snake>]);
        }
    };
    ($( $fixture:ident ),+ ) => {
        $(
            add_many_tests!($fixture);
        )+
    };
}

add_many_tests!(
    Linear,
    BranchEven,
    BranchUneven,
    BranchWide,
    MergeEven,
    MergeUneven,
    UnsharedMergeEven,
    UnsharedMergeUneven,
    ManyDiamonds
);
