/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use anyhow::Error;
use assert_matches::assert_matches;
use bonsai_globalrev_mapping::{
    add_globalrevs, AddGlobalrevsErrorKind, BonsaiGlobalrevMapping, BonsaiGlobalrevMappingEntry,
    BonsaisOrGlobalrevs, SqlBonsaiGlobalrevMapping,
};
use futures_preview::compat::Future01CompatExt;
use mercurial_types_mocks::globalrev::*;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql::Connection;
use sql_ext::{open_sqlite_in_memory, SqlConstructors};

#[fbinit::test]
async fn test_add_and_get() -> Result<(), Error> {
    let mapping = SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?;

    let entry = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };

    mapping.bulk_import(&vec![entry.clone()]).compat().await?;

    let result = mapping
        .get(
            REPO_ZERO,
            BonsaisOrGlobalrevs::Bonsai(vec![bonsai::ONES_CSID]),
        )
        .compat()
        .await?;
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping
        .get_globalrev_from_bonsai(REPO_ZERO, bonsai::ONES_CSID)
        .compat()
        .await?;
    assert_eq!(result, Some(GLOBALREV_ZERO));
    let result = mapping
        .get_bonsai_from_globalrev(REPO_ZERO, GLOBALREV_ZERO)
        .compat()
        .await?;
    assert_eq!(result, Some(bonsai::ONES_CSID));

    Ok(())
}

#[fbinit::test]
async fn test_bulk_import() -> Result<(), Error> {
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
        .bulk_import(&vec![entry1.clone(), entry2.clone(), entry3.clone()])
        .compat()
        .await?;

    Ok(())
}

#[fbinit::test]
async fn test_missing() -> Result<(), Error> {
    let mapping = SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?;

    let result = mapping
        .get(
            REPO_ZERO,
            BonsaisOrGlobalrevs::Bonsai(vec![bonsai::ONES_CSID]),
        )
        .compat()
        .await?;

    assert_eq!(result, vec![]);

    Ok(())
}

#[fbinit::test]
async fn test_get_max() -> Result<(), Error> {
    let mapping = SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()?;

    assert_eq!(None, mapping.get_max(REPO_ZERO).compat().await?);

    let e0 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::ONES_CSID,
        globalrev: GLOBALREV_ZERO,
    };
    mapping.bulk_import(&[e0.clone()]).compat().await?;
    assert_eq!(
        Some(GLOBALREV_ZERO),
        mapping.get_max(REPO_ZERO).compat().await?
    );

    let e1 = BonsaiGlobalrevMappingEntry {
        repo_id: REPO_ZERO,
        bcs_id: bonsai::TWOS_CSID,
        globalrev: GLOBALREV_ONE,
    };
    mapping.bulk_import(&[e1.clone()]).compat().await?;
    assert_eq!(
        Some(GLOBALREV_ONE),
        mapping.get_max(REPO_ZERO).compat().await?
    );

    Ok(())
}

#[fbinit::test]
async fn test_add_globalrevs() -> Result<(), Error> {
    let conn = open_sqlite_in_memory()?;
    conn.execute_batch(SqlBonsaiGlobalrevMapping::get_up_query())?;
    let conn = Connection::with_sqlite(conn);

    let mapping =
        SqlBonsaiGlobalrevMapping::from_connections(conn.clone(), conn.clone(), conn.clone());

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

    let txn = conn.start_transaction().compat().await?;
    let txn = add_globalrevs(txn, &[e0.clone()]).await?;
    txn.commit().compat().await?;

    assert_eq!(
        Some(GLOBALREV_ZERO),
        mapping
            .get_globalrev_from_bonsai(REPO_ZERO, bonsai::ONES_CSID)
            .compat()
            .await?
    );

    let txn = conn.start_transaction().compat().await?;
    let txn = add_globalrevs(txn, &[e1.clone()]).await?;
    txn.commit().compat().await?;

    assert_eq!(
        Some(GLOBALREV_ONE),
        mapping
            .get_globalrev_from_bonsai(REPO_ZERO, bonsai::TWOS_CSID)
            .compat()
            .await?
    );

    // Inserting duplicates fails

    let txn = conn.start_transaction().compat().await?;
    let res = async move {
        let txn = add_globalrevs(txn, &[e1.clone()]).await?;
        txn.commit().compat().await?;
        Result::<_, AddGlobalrevsErrorKind>::Ok(())
    }
        .await;
    assert_matches!(res, Err(AddGlobalrevsErrorKind::Conflict));

    assert_eq!(
        Some(GLOBALREV_ONE),
        mapping
            .get_globalrev_from_bonsai(REPO_ZERO, bonsai::TWOS_CSID)
            .compat()
            .await?
    );

    Ok(())
}
