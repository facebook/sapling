/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types_mocks::repo::REPO_ZERO;
use mutable_counters::MutableCounters;
use mutable_counters::SqlMutableCounters;
use mutable_counters::SqlMutableCountersBuilder;
use sql_construct::SqlConstruct;

fn create_db() -> Result<SqlMutableCounters> {
    Ok(SqlMutableCountersBuilder::with_sqlite_in_memory()?.build(REPO_ZERO))
}

#[fbinit::test]
async fn test_counter_simple(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let mutable_counters = create_db()?;

    mutable_counters
        .set_counter(&ctx, "counter", 1, None)
        .await?;
    assert_eq!(
        mutable_counters.get_counter(&ctx, "counter").await?,
        Some(1)
    );

    mutable_counters
        .set_counter(&ctx, "counter", 2, None)
        .await?;
    assert_eq!(
        mutable_counters.get_counter(&ctx, "counter").await?,
        Some(2)
    );

    Ok(())
}

#[fbinit::test]
async fn test_counter_conditional_update(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let mutable_counters = create_db()?;

    mutable_counters
        .set_counter(&ctx, "counter", 1, None)
        .await?;
    assert_eq!(
        mutable_counters.get_counter(&ctx, "counter").await?,
        Some(1)
    );

    assert!(
        mutable_counters
            .set_counter(&ctx, "counter", 2, Some(1))
            .await?
    );
    assert_eq!(
        mutable_counters.get_counter(&ctx, "counter").await?,
        Some(2)
    );

    // Not updated because prev_value is incorrect
    assert!(
        !mutable_counters
            .set_counter(&ctx, "counter", 3, Some(1))
            .await?
    );
    assert_eq!(
        mutable_counters.get_counter(&ctx, "counter").await?,
        Some(2)
    );

    // Attempt to update another counter, make sure it doesn't touch first counter
    // and update doesn't succeed as previous value is not set.
    assert!(
        !mutable_counters
            .set_counter(&ctx, "counter2", 3, Some(2))
            .await?,
    );
    assert_eq!(
        mutable_counters.get_counter(&ctx, "counter").await?,
        Some(2)
    );
    assert_eq!(mutable_counters.get_counter(&ctx, "counter2").await?, None);

    Ok(())
}
