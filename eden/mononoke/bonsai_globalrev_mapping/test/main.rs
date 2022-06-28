/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use assert_matches::assert_matches;
use context::CoreContext;
use fbinit::FacebookInit;
use mercurial_types_mocks::globalrev::*;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_ext::open_sqlite_in_memory;
use sql_ext::SqlConnections;

use bonsai_globalrev_mapping::add_globalrevs;
use bonsai_globalrev_mapping::AddGlobalrevsErrorKind;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingEntry;
use bonsai_globalrev_mapping::BonsaisOrGlobalrevs;
use bonsai_globalrev_mapping::CachingBonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::SqlBonsaiGlobalrevMappingBuilder;

#[fbinit::test]
async fn test_add_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let entry = BonsaiGlobalrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };

    mapping.bulk_import(&ctx, &[entry.clone()]).await?;

    let result = mapping
        .get(&ctx, BonsaisOrGlobalrevs::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;
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

    assert_eq!(result, vec![]);

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
    let mapping = SqlBonsaiGlobalrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);
    let caching = CachingBonsaiGlobalrevMapping::new_test(mapping.clone());
    let store = caching
        .cachelib()
        .mock_store()
        .expect("new_test gives us a MockStore");

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
        Some(GLOBALREV_TWO)
    );
    assert_eq!(store.stats().gets, 3);
    assert_eq!(store.stats().hits, 1);
    assert_eq!(store.stats().sets, 2);

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, GLOBALREV_ONE)
            .await?,
        Some(bonsai::ONES_CSID)
    );

    assert_eq!(store.stats().gets, 4);
    assert_eq!(store.stats().hits, 1);
    assert_eq!(store.stats().sets, 3);

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, GLOBALREV_ONE)
            .await?,
        Some(bonsai::ONES_CSID)
    );

    assert_eq!(store.stats().gets, 5);
    assert_eq!(store.stats().hits, 2);
    assert_eq!(store.stats().sets, 3);

    Ok(())
}
