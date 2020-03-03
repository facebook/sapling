/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use assert_matches::assert_matches;
use bonsai_git_mapping::{
    bulk_add_git_mapping_in_transaction, AddGitMappingErrorKind, BonsaiGitMapping,
    BonsaiGitMappingEntry, BonsaisOrGitShas, SqlBonsaiGitMappingConnection,
};
use futures::compat::Future01CompatExt;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::hash::*;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql::Connection;
use sql_ext::{open_sqlite_in_memory, SqlConstructors};

#[fbinit::test]
async fn test_add_and_get() -> Result<(), Error> {
    let mapping = SqlBonsaiGitMappingConnection::with_sqlite_in_memory()?.with_repo_id(REPO_ZERO);

    let entry = BonsaiGitMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };

    mapping.bulk_add(&vec![entry.clone()]).await?;

    let result = mapping
        .get(BonsaisOrGitShas::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;
    assert_eq!(result, vec![entry.clone()]);
    let result = mapping.get_git_sha1_from_bonsai(bonsai::ONES_CSID).await?;
    assert_eq!(result, Some(ONES_GIT_SHA1));
    let result = mapping.get_bonsai_from_git_sha1(ONES_GIT_SHA1).await?;
    assert_eq!(result, Some(bonsai::ONES_CSID));

    Ok(())
}

#[fbinit::test]
async fn test_bulk_add() -> Result<(), Error> {
    let mapping = SqlBonsaiGitMappingConnection::with_sqlite_in_memory()?.with_repo_id(REPO_ZERO);

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
        .bulk_add(&vec![entry1.clone(), entry2.clone(), entry3.clone()])
        .await?;

    Ok(())
}

#[fbinit::test]
async fn test_missing() -> Result<(), Error> {
    let mapping = SqlBonsaiGitMappingConnection::with_sqlite_in_memory()?.with_repo_id(REPO_ZERO);

    let result = mapping
        .get(BonsaisOrGitShas::Bonsai(vec![bonsai::ONES_CSID]))
        .await?;

    assert_eq!(result, vec![]);

    Ok(())
}

#[fbinit::test]
async fn test_add_with_transaction() -> Result<(), Error> {
    let conn = open_sqlite_in_memory()?;
    conn.execute_batch(SqlBonsaiGitMappingConnection::get_up_query())?;
    let conn = Connection::with_sqlite(conn);

    let mapping =
        SqlBonsaiGitMappingConnection::from_connections(conn.clone(), conn.clone(), conn.clone())
            .with_repo_id(REPO_ZERO);

    let entry1 = BonsaiGitMappingEntry {
        bcs_id: bonsai::ONES_CSID,
        git_sha1: ONES_GIT_SHA1,
    };
    let entry2 = BonsaiGitMappingEntry {
        bcs_id: bonsai::TWOS_CSID,
        git_sha1: TWOS_GIT_SHA1,
    };

    let txn = conn.start_transaction().compat().await?;
    bulk_add_git_mapping_in_transaction(txn, &REPO_ZERO, &[entry1.clone()])
        .await?
        .commit()
        .compat()
        .await?;

    assert_eq!(
        Some(ONES_GIT_SHA1),
        mapping.get_git_sha1_from_bonsai(bonsai::ONES_CSID).await?
    );

    let txn = conn.start_transaction().compat().await?;
    bulk_add_git_mapping_in_transaction(txn, &REPO_ZERO, &[entry2.clone()])
        .await?
        .commit()
        .compat()
        .await?;

    assert_eq!(
        Some(TWOS_GIT_SHA1),
        mapping.get_git_sha1_from_bonsai(bonsai::TWOS_CSID).await?
    );

    // Inserting duplicates fails
    let txn = conn.start_transaction().compat().await?;
    let res = async move {
        bulk_add_git_mapping_in_transaction(txn, &REPO_ZERO, &[entry2.clone()])
            .await?
            .commit()
            .compat()
            .await?;
        Result::<_, AddGitMappingErrorKind>::Ok(())
    }
    .await;
    assert_matches!(res, Err(AddGitMappingErrorKind::Conflict(_)));

    assert_eq!(
        Some(TWOS_GIT_SHA1),
        mapping.get_git_sha1_from_bonsai(bonsai::TWOS_CSID).await?
    );

    Ok(())
}
