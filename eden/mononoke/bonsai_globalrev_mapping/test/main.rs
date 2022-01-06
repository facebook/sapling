/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use assert_matches::assert_matches;
use context::CoreContext;
use fbinit::FacebookInit;
use mercurial_types_mocks::globalrev::*;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::{REPO_ONE, REPO_ZERO};
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_ext::{open_sqlite_in_memory, SqlConnections};
use std::sync::Arc;

use bonsai_globalrev_mapping::{
    add_globalrevs, AddGlobalrevsErrorKind, BonsaiGlobalrevMapping, BonsaiGlobalrevMappingEntry,
    BonsaisOrGlobalrevs, CachingBonsaiGlobalrevMapping, SqlBonsaiGlobalrevMapping,
};

#[fbinit::test]
async fn test_add_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?;

    let entry = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };

    mapping.bulk_import(&ctx, &[entry.clone()]).await?;

    let result = mapping
        .get(
            &ctx,
            REPO_ZERO,
            BonsaisOrGlobalrevs::Bonsai(vec![bonsai::ONES_CSID]),
        )
        .await?;
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_globalrev_from_bonsai(&ctx, REPO_ZERO, bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, Some(GLOBALREV_ZERO));
    let result = mapping
        .get_bonsai_from_globalrev(&ctx, REPO_ZERO, GLOBALREV_ZERO)
        .await?;
    assert_eq!(result, Some(bonsai::ONES_CSID));

    Ok(())
}

#[fbinit::test]
async fn test_bulk_import(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?;

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

    mapping
        .bulk_import(&ctx, &[entry1.clone(), entry2.clone(), entry3.clone()])
        .await?;

    Ok(())
}

#[fbinit::test]
async fn test_missing(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?;

    let result = mapping
        .get(
            &ctx,
            REPO_ZERO,
            BonsaisOrGlobalrevs::Bonsai(vec![bonsai::ONES_CSID]),
        )
        .await?;

    assert_eq!(result, vec![]);

    Ok(())
}

#[fbinit::test]
async fn test_get_max(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?;

    assert_eq!(None, mapping.get_max(&ctx, REPO_ZERO).await?);

    let e0 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };
    mapping.bulk_import(&ctx, &[e0.clone()]).await?;
    assert_eq!(
        Some(GLOBALREV_ZERO),
        mapping.get_max(&ctx, REPO_ZERO).await?
    );

    let e1 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_ONE,
    };
    mapping.bulk_import(&ctx, &[e1.clone()]).await?;
    assert_eq!(Some(GLOBALREV_ONE), mapping.get_max(&ctx, REPO_ZERO).await?);

    Ok(())
}

#[fbinit::test]
async fn test_add_globalrevs(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let conn = open_sqlite_in_memory()?;
    conn.execute_batch(SqlBonsaiGlobalrevMapping::CREATION_QUERY)?;
    let conn = Connection::with_sqlite(conn);

    let mapping =
        SqlBonsaiGlobalrevMapping::from_sql_connections(SqlConnections::new_single(conn.clone()));

    let e0 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };

    let e1 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_ONE,
    };

    let txn = conn.start_transaction().await?;
    let txn = add_globalrevs(txn, &[e0.clone()]).await?;
    txn.commit().await?;

    assert_eq!(
        Some(GLOBALREV_ZERO),
        mapping
            .get_globalrev_from_bonsai(&ctx, REPO_ZERO, bonsai::ONES_CSID)
            .await?
    );

    let txn = conn.start_transaction().await?;
    let txn = add_globalrevs(txn, &[e1.clone()]).await?;
    txn.commit().await?;

    assert_eq!(
        Some(GLOBALREV_ONE),
        mapping
            .get_globalrev_from_bonsai(&ctx, REPO_ZERO, bonsai::TWOS_CSID)
            .await?
    );

    // Inserting duplicates fails

    let txn = conn.start_transaction().await?;
    let res = async move {
        let txn = add_globalrevs(txn, &[e1.clone()]).await?;
        txn.commit().await?;
        Result::<_, AddGlobalrevsErrorKind>::Ok(())
    }
    .await;
    assert_matches!(res, Err(AddGlobalrevsErrorKind::Conflict));

    assert_eq!(
        Some(GLOBALREV_ONE),
        mapping
            .get_globalrev_from_bonsai(&ctx, REPO_ZERO, bonsai::TWOS_CSID)
            .await?
    );

    Ok(())
}

#[fbinit::test]
async fn test_closest_globalrev(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?;

    let e0 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ONE,
    };

    let e1 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_TWO,
    };

    mapping.bulk_import(&ctx, &[e0, e1]).await?;

    assert_eq!(
        mapping
            .get_closest_globalrev(&ctx, REPO_ZERO, GLOBALREV_ZERO)
            .await?,
        None
    );

    assert_eq!(
        mapping
            .get_closest_globalrev(&ctx, REPO_ZERO, GLOBALREV_ONE)
            .await?,
        Some(GLOBALREV_ONE)
    );

    assert_eq!(
        mapping
            .get_closest_globalrev(&ctx, REPO_ZERO, GLOBALREV_TWO)
            .await?,
        Some(GLOBALREV_TWO)
    );

    assert_eq!(
        mapping
            .get_closest_globalrev(&ctx, REPO_ZERO, GLOBALREV_THREE)
            .await?,
        Some(GLOBALREV_TWO)
    );

    assert_eq!(
        mapping
            .get_closest_globalrev(&ctx, REPO_ONE, GLOBALREV_THREE)
            .await?,
        None,
    );

    Ok(())
}

#[fbinit::test]
async fn test_caching(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = Arc::new(SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?);
    let caching = CachingBonsaiGlobalrevMapping::new_test(mapping.clone());

    let store = caching
        .cachelib()
        .mock_store()
        .expect("new_test gives us a MockStore");

    let e0 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ONE,
    };

    let e1 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_TWO,
    };

    let ex = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ONE,
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_ONE,
    };

    mapping.bulk_import(&ctx, &[e0, e1, ex]).await?;

    assert_eq!(
        caching
            .get_globalrev_from_bonsai(&ctx, REPO_ZERO, bonsai::ONES_CSID)
            .await?,
        Some(GLOBALREV_ONE)
    );

    assert_eq!(store.stats().gets, 1);
    assert_eq!(store.stats().hits, 0);
    assert_eq!(store.stats().sets, 1);

    assert_eq!(
        caching
            .get_globalrev_from_bonsai(&ctx, REPO_ZERO, bonsai::ONES_CSID)
            .await?,
        Some(GLOBALREV_ONE)
    );

    assert_eq!(store.stats().gets, 2);
    assert_eq!(store.stats().hits, 1);
    assert_eq!(store.stats().sets, 1);

    assert_eq!(
        caching
            .get_globalrev_from_bonsai(&ctx, REPO_ZERO, bonsai::TWOS_CSID)
            .await?,
        Some(GLOBALREV_TWO)
    );

    assert_eq!(
        caching
            .get_globalrev_from_bonsai(&ctx, REPO_ONE, bonsai::TWOS_CSID)
            .await?,
        Some(GLOBALREV_ONE)
    );

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, REPO_ZERO, GLOBALREV_ONE)
            .await?,
        Some(bonsai::ONES_CSID)
    );

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, REPO_ZERO, GLOBALREV_TWO)
            .await?,
        Some(bonsai::TWOS_CSID)
    );

    assert_eq!(
        caching
            .get_bonsai_from_globalrev(&ctx, REPO_ONE, GLOBALREV_ONE)
            .await?,
        Some(bonsai::TWOS_CSID)
    );

    Ok(())
}
