/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use anyhow::Error;
use bonsai_globalrev_mapping::{
    BonsaiGlobalrevMapping, BonsaiGlobalrevMappingEntry, BonsaisOrGlobalrevs,
    SqlBonsaiGlobalrevMapping,
};
use futures_preview::compat::Future01CompatExt;
use mercurial_types_mocks::globalrev::*;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql_ext::SqlConstructors;

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
