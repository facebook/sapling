/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use assert_matches::assert_matches;
use bonsai_globalrev_mapping::add_globalrevs;
use bonsai_globalrev_mapping::AddGlobalrevsErrorKind;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingEntries;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingEntry;
use bonsai_globalrev_mapping::BonsaisOrGlobalrevs;
use bonsai_globalrev_mapping::CachingBonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::SqlBonsaiGlobalrevMappingBuilder;
use context::CoreContext;
use fbinit::FacebookInit;
use mercurial_types_mocks::globalrev::*;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_ext::open_sqlite_in_memory;
use sql_ext::SqlConnections;

#[fbinit::test]
async fn test_add_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let entry = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };

    mapping.bulk_import(&ctx, &[entry.clone()]).await?;

    let result: Vec<_> = mapping
        .get(&ctx, BonsaisOrGlobalrevs::Bonsai(vec![bonsai::ONES_CSID]))
        .await?
        .into();
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_globalrev_from_bonsai(&ctx, bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, Some(GLOBALREV_ZERO));
    let result = mapping
        .get_bonsai_from_globalrev(&ctx, GLOBALREV_ZERO)
        .await?;
    assert_eq!(result, Some(bonsai::ONES_CSID));

    Ok(())
}

#[fbinit::test]
async fn test_bulk_import(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let entry1 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };
    let entry2 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_ONE,
    };
    let entry3 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::THREES_CSID,
        globalrev: GLOBALREV_TWO,
    };

    mapping
        .bulk_import(&ctx, &[entry1.clone(), entry2.clone(), entry3.clone()])
        .await?;

    Ok(())
}

#[fbinit::test]
async fn test_missing(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let result = mapping
        .get(&ctx, BonsaisOrGlobalrevs::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;

    assert_eq!(result, BonsaiGlobalrevMappingEntries::empty());

    Ok(())
}

#[fbinit::test]
async fn test_get_max(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    assert_eq!(None, mapping.get_max(&ctx).await?);

    let e0 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };
    mapping.bulk_import(&ctx, &[e0.clone()]).await?;
    assert_eq!(Some(GLOBALREV_ZERO), mapping.get_max(&ctx).await?);

    let e1 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_ONE,
    };
    mapping.bulk_import(&ctx, &[e1.clone()]).await?;
    assert_eq!(Some(GLOBALREV_ONE), mapping.get_max(&ctx).await?);

    Ok(())
}

#[fbinit::test]
async fn test_add_globalrevs(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let conn = open_sqlite_in_memory()?;
    conn.execute_batch(SqlBonsaiGlobalrevMappingBuilder::CREATION_QUERY)?;
    let conn = Connection::with_sqlite(conn);

    let mapping = SqlBonsaiGlobalrevMappingBuilder::from_sql_connections(
        SqlConnections::new_single(conn.clone()),
    )
    .build(REPO_ZERO);

    let e0 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };

    let e1 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_ONE,
    };

    let txn = conn.start_transaction().await?;
    let txn = add_globalrevs(txn, REPO_ZERO, &[e0.clone()]).await?;
    txn.commit().await?;

    assert_eq!(
        Some(GLOBALREV_ZERO),
        mapping
            .get_globalrev_from_bonsai(&ctx, bonsai::ONES_CSID)
            .await?
    );

    let txn = conn.start_transaction().await?;
    let txn = add_globalrevs(txn, REPO_ZERO, &[e1.clone()]).await?;
    txn.commit().await?;

    assert_eq!(
        Some(GLOBALREV_ONE),
        mapping
            .get_globalrev_from_bonsai(&ctx, bonsai::TWOS_CSID)
            .await?
    );

    // Inserting duplicates fails

    let txn = conn.start_transaction().await?;
    let res = async move {
        let txn = add_globalrevs(txn, REPO_ZERO, &[e1.clone()]).await?;
        txn.commit().await?;
        Result::<_, AddGlobalrevsErrorKind>::Ok(())
    }
    .await;
    assert_matches!(res, Err(AddGlobalrevsErrorKind::Conflict));

    assert_eq!(
        Some(GLOBALREV_ONE),
        mapping
            .get_globalrev_from_bonsai(&ctx, bonsai::TWOS_CSID)
            .await?
    );

    Ok(())
}

#[fbinit::test]
async fn test_closest_globalrev(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let e0 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ONE,
    };

    let e1 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_TWO,
    };

    mapping.bulk_import(&ctx, &[e0, e1]).await?;

    assert_eq!(
        mapping.get_closest_globalrev(&ctx, GLOBALREV_ZERO).await?,
        None
    );

    assert_eq!(
        mapping.get_closest_globalrev(&ctx, GLOBALREV_ONE).await?,
        Some(GLOBALREV_ONE)
    );

    assert_eq!(
        mapping.get_closest_globalrev(&ctx, GLOBALREV_TWO).await?,
        Some(GLOBALREV_TWO)
    );

    assert_eq!(
        mapping.get_closest_globalrev(&ctx, GLOBALREV_THREE).await?,
        Some(GLOBALREV_TWO)
    );

    assert_eq!(
        mapping.get_closest_globalrev(&ctx, GLOBALREV_ZERO).await?,
        None
    );

    Ok(())
}

#[fbinit::test]
async fn test_caching(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping =
        Arc::new(SqlBonsaiGlobalrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO));
    let caching = CachingBonsaiGlobalrevMapping::new_test(mapping.clone());
    let store = caching
        .cachelib()
        .mock_store()
        .expect("new_test gives us a MockStore");

    let e0 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ONE,
    };

    // Gap: e1 with (bonsai::TWOS_CSID, GLOBALREV_TWO)

    let e2 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::THREES_CSID,
        globalrev: GLOBALREV_THREE,
    };

    // There is a gap here for e1. Caching should still work for that gap
    mapping.bulk_import(&ctx, &[e0, e2]).await?;

    assert_eq!(
        caching
            .get_globalrev_from_bonsai(&ctx, bonsai::ONES_CSID)
            .await?,
        Some(GLOBALREV_ONE)
    );

    assert_eq!(store.stats().gets, 1);
    assert_eq!(store.stats().hits, 0);
    assert_eq!(store.stats().sets, 1);

    assert_eq!(
        caching
            .get_globalrev_from_bonsai(&ctx, bonsai::ONES_CSID)
            .await?,
        Some(GLOBALREV_ONE)
    );

    assert_eq!(store.stats().gets, 2);
    assert_eq!(store.stats().hits, 1);
    assert_eq!(store.stats().sets, 1);

    assert_eq!(
        caching
            .get_globalrev_from_bonsai(&ctx, bonsai::TWOS_CSID)
            .await?,
        None,
    );
    assert_eq!(store.stats().gets, 3);
    assert_eq!(store.stats().hits, 1);
    // We don't set the cache when trying to get a globalrev for a bonsai that doesn't exist:
    // If a bonsai doesn't exist, we can't know for sure that it won't exist in the future for
    // this repo
    assert_eq!(store.stats().sets, 1);

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, GLOBALREV_ONE)
            .await?,
        Some(bonsai::ONES_CSID)
    );

    assert_eq!(store.stats().gets, 4);
    assert_eq!(store.stats().hits, 1);
    assert_eq!(store.stats().sets, 2);

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, GLOBALREV_ONE)
            .await?,
        Some(bonsai::ONES_CSID)
    );

    assert_eq!(store.stats().gets, 5);
    assert_eq!(store.stats().hits, 2);
    assert_eq!(store.stats().sets, 2);

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, GLOBALREV_TWO)
            .await?,
        None
    );

    assert_eq!(store.stats().gets, 6);
    assert_eq!(store.stats().hits, 2);
    // We are caching the globalrev gap here
    assert_eq!(store.stats().sets, 3);

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, GLOBALREV_TWO)
            .await?,
        None
    );

    assert_eq!(store.stats().gets, 7);
    // We are hitting the cache for the globalrev gap here
    assert_eq!(store.stats().hits, 3);
    assert_eq!(store.stats().sets, 3);

    assert_eq!(
        caching
            .get_globalrev_from_bonsai(&ctx, bonsai::THREES_CSID)
            .await?,
        Some(GLOBALREV_THREE)
    );
    assert_eq!(store.stats().gets, 8);
    assert_eq!(store.stats().hits, 3);
    assert_eq!(store.stats().sets, 4);

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, GLOBALREV_THREE)
            .await?,
        Some(bonsai::THREES_CSID)
    );

    assert_eq!(store.stats().gets, 9);
    assert_eq!(store.stats().hits, 3);
    assert_eq!(store.stats().sets, 5);

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, GLOBALREV_THREE)
            .await?,
        Some(bonsai::THREES_CSID)
    );

    assert_eq!(store.stats().gets, 10);
    assert_eq!(store.stats().hits, 4);
    assert_eq!(store.stats().sets, 5);

    Ok(())
}

#[fbinit::test]
async fn test_globalrev_gaps(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping =
        Arc::new(SqlBonsaiGlobalrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO));
    let caching = CachingBonsaiGlobalrevMapping::new_test(mapping.clone());
    let store = caching
        .cachelib()
        .mock_store()
        .expect("new_test gives us a MockStore");

    let e0 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::AS_CSID,
        globalrev: GLOBALREV_ZERO,
    };
    // Gap: e1 with (bonsai::BS_CSID, GLOBALREV_ONE)
    let e2 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::CS_CSID,
        globalrev: GLOBALREV_TWO,
    };

    // There is a gap here for e1
    mapping.bulk_import(&ctx, &[e0.clone(), e2.clone()]).await?;
    let mut result: Vec<_> = caching
        .get(
            &ctx,
            BonsaisOrGlobalrevs::Globalrev(vec![GLOBALREV_ZERO, GLOBALREV_ONE, GLOBALREV_TWO]),
        )
        .await?
        .into();
    result.sort_by(|left, right| left.globalrev.partial_cmp(&right.globalrev).unwrap());
    assert_eq!(result, vec![e0.clone(), e2.clone()]);

    // We should have had no hits yet as we asked for 3 different values.
    // We should have set three cache values (including one for the gap)
    assert_eq!(store.stats().gets, 3);
    assert_eq!(store.stats().hits, 0);
    assert_eq!(store.stats().sets, 3);
    let result: Vec<_> = caching
        .get(&ctx, BonsaisOrGlobalrevs::Globalrev(vec![GLOBALREV_THREE]))
        .await?
        .into();
    assert_eq!(result, vec![]);

    assert_eq!(store.stats().gets, 4);
    assert_eq!(store.stats().hits, 0);
    // We've not set this cache entry as there is no larger globalrev in the table,
    // so it might be be set later
    assert_eq!(store.stats().sets, 3);

    // Gap: e3 with (bonsai::CS_CSID, GLOBALREV_THREE)
    let e4 = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::ES_CSID,
        globalrev: GLOBALREV_FOUR,
    };

    mapping.bulk_import(&ctx, &[e4.clone()]).await?;
    assert_eq!(Some(GLOBALREV_FOUR), mapping.get_max(&ctx).await?);
    let result: Vec<_> = caching
        .get(&ctx, BonsaisOrGlobalrevs::Globalrev(vec![GLOBALREV_THREE]))
        .await?
        .into();
    assert_eq!(result, vec![]);

    assert_eq!(store.stats().gets, 5);
    assert_eq!(store.stats().hits, 0);
    // Now there is a larger globalrev in the mapping table, we know that this is a gap
    // and not merely a missing entry, so we can cache the absence
    assert_eq!(store.stats().sets, 4);

    let result: Vec<_> = caching
        .get(&ctx, BonsaisOrGlobalrevs::Globalrev(vec![GLOBALREV_THREE]))
        .await?
        .into();
    assert_eq!(result, vec![]);

    assert_eq!(store.stats().gets, 6);
    // When we try to access the gap a second time, we get a cache hit as expected
    assert_eq!(store.stats().hits, 1);
    assert_eq!(store.stats().sets, 4);

    let mut result: Vec<_> = caching
        .get(
            &ctx,
            BonsaisOrGlobalrevs::Globalrev(vec![
                GLOBALREV_ZERO,
                GLOBALREV_ONE,
                GLOBALREV_TWO,
                GLOBALREV_THREE,
                GLOBALREV_FOUR,
                GLOBALREV_FIVE,
            ]),
        )
        .await?
        .into();
    result.sort_by(|left, right| left.globalrev.partial_cmp(&right.globalrev).unwrap());
    assert_eq!(result, vec![e0.clone(), e2.clone(), e4.clone()]);

    assert_eq!(store.stats().gets, 12); // 6 new gets
    assert_eq!(store.stats().hits, 5); // 4 new hits: from zero to three, including gaps 
    assert_eq!(store.stats().sets, 5); // 1 new set: four

    Ok(())
}
