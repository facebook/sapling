/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types_mocks::changesetid;
use mononoke_types_mocks::repo;
use sql_construct::SqlConstruct;
use sql_ext::Connection;
use sql_ext::open_sqlite_in_memory;

use crate::PushrebaseMutationMappingEntry;
use crate::SqlPushrebaseMutationMappingConnection;
use crate::add_pushrebase_mapping;
use crate::get_prepushrebase_ids;

#[mononoke::fbinit_test]
async fn test_add_and_get(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let conn = open_sqlite_in_memory()?;
    conn.execute_batch(SqlPushrebaseMutationMappingConnection::CREATION_QUERY)?;
    let conn = Connection::with_sqlite(conn)?;

    let entries = vec![
        PushrebaseMutationMappingEntry::new(
            repo::REPO_ZERO,
            changesetid::ONES_CSID,
            changesetid::TWOS_CSID,
        ),
        PushrebaseMutationMappingEntry::new(
            repo::REPO_ONE,
            changesetid::ONES_CSID,
            changesetid::TWOS_CSID,
        ),
        PushrebaseMutationMappingEntry::new(
            repo::REPO_ONE,
            changesetid::TWOS_CSID,
            changesetid::TWOS_CSID,
        ),
        PushrebaseMutationMappingEntry::new(
            repo::REPO_ONE,
            changesetid::ONES_CSID,
            changesetid::THREES_CSID,
        ),
    ];

    let txn = conn.start_transaction(ctx.sql_query_telemetry()).await?;
    let txn = add_pushrebase_mapping(txn, &entries).await?;
    txn.commit().await?;

    let mut prepushrebase_ids =
        get_prepushrebase_ids(&ctx, &conn, repo::REPO_ONE, changesetid::TWOS_CSID).await?;
    prepushrebase_ids.sort();

    assert_eq!(
        prepushrebase_ids,
        vec![changesetid::ONES_CSID, changesetid::TWOS_CSID]
    );

    Ok(())
}
