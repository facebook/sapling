/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;
use mononoke_types_mocks::svnrev::*;
use sql_construct::SqlConstruct;
use std::sync::Arc;

use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMappingEntry;
use bonsai_svnrev_mapping::BonsaisOrSvnrevs;
use bonsai_svnrev_mapping::CachingBonsaiSvnrevMapping;
use bonsai_svnrev_mapping::SqlBonsaiSvnrevMappingBuilder;

#[fbinit::test]
async fn test_add_and_get(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiSvnrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let entry = BonsaiSvnrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        svnrev: SVNREV_ZERO,
    };

    mapping.bulk_import(&ctx, &[entry.clone()]).await?;

    let result = mapping
        .get(&ctx, BonsaisOrSvnrevs::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_svnrev_from_bonsai(&ctx, bonsai::ONES_CSID)
        .await?;
    assert_eq!(result, Some(SVNREV_ZERO));
    let result = mapping.get_bonsai_from_svnrev(&ctx, SVNREV_ZERO).await?;
    assert_eq!(result, Some(bonsai::ONES_CSID));

    Ok(())
}

#[fbinit::test]
async fn test_bulk_import(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiSvnrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let entry1 = BonsaiSvnrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        svnrev: SVNREV_ZERO,
    };
    let entry2 = BonsaiSvnrevMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        svnrev: SVNREV_ONE,
    };
    let entry3 = BonsaiSvnrevMappingEntry {
        bcs_id: bonsai::THREES_CSID,
        svnrev: SVNREV_TWO,
    };

    mapping
        .bulk_import(&ctx, &[entry1.clone(), entry2.clone(), entry3.clone()])
        .await?;

    Ok(())
}

#[fbinit::test]
async fn test_missing(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping = SqlBonsaiSvnrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO);

    let result = mapping
        .get(&ctx, BonsaisOrSvnrevs::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;

    assert_eq!(result, vec![]);

    Ok(())
}

#[fbinit::test]
async fn test_caching(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mapping =
        Arc::new(SqlBonsaiSvnrevMappingBuilder::with_sqlite_in_memory()?.build(REPO_ZERO));
    let caching = CachingBonsaiSvnrevMapping::new_test(mapping.clone());

    let store = caching
        .cachelib()
        .mock_store()
        .expect("new_test gives us a MockStore");

    let e0 = BonsaiSvnrevMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        svnrev: SVNREV_ONE,
    };

    let e1 = BonsaiSvnrevMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        svnrev: SVNREV_TWO,
    };

    mapping.bulk_import(&ctx, &[e0, e1]).await?;

    assert_eq!(
        caching
            .get_svnrev_from_bonsai(&ctx, bonsai::ONES_CSID)
            .await?,
        Some(SVNREV_ONE)
    );

    assert_eq!(store.stats().gets, 1);
    assert_eq!(store.stats().hits, 0);
    assert_eq!(store.stats().sets, 1);

    assert_eq!(
        caching
            .get_svnrev_from_bonsai(&ctx, bonsai::ONES_CSID)
            .await?,
        Some(SVNREV_ONE)
    );

    assert_eq!(store.stats().gets, 2);
    assert_eq!(store.stats().hits, 1);
    assert_eq!(store.stats().sets, 1);

    assert_eq!(
        caching
            .get_svnrev_from_bonsai(&ctx, bonsai::TWOS_CSID)
            .await?,
        Some(SVNREV_TWO)
    );

    assert_eq!(
        caching.get_bonsai_from_svnrev(&ctx, SVNREV_ONE).await?,
        Some(bonsai::ONES_CSID)
    );

    assert_eq!(
        caching.get_bonsai_from_svnrev(&ctx, SVNREV_TWO).await?,
        Some(bonsai::TWOS_CSID)
    );

    Ok(())
}
