/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use assert_matches::assert_matches;
use bonsai_git_mapping::AddGitMappingErrorKind;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaisOrGitShas;
use bonsai_git_mapping::SqlBonsaiGitMappingBuilder;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types_mocks::changesetid as bonsai;
use mononoke_types_mocks::hash::*;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_ext::open_sqlite_in_memory;
use sql_ext::SqlConnections;

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
    let res = {
        let ctx = ctx.clone();
        let mapping = mapping.clone();
        async move {
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
