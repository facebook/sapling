/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{Error, Result};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures_old::Future;
use mononoke_types_mocks::repo::{REPO_ONE, REPO_ZERO};
use mutable_counters::{MutableCounters, SqlMutableCounters};
use sql_construct::SqlConstruct;

fn create_db() -> SqlMutableCounters {
    SqlMutableCounters::with_sqlite_in_memory().unwrap()
}

fn run_future<F, I>(runtime: &tokio::runtime::Runtime, future: F) -> Result<I>
where
    F: Future<Item = I, Error = Error> + Send + 'static,
    I: Send + 'static,
{
    runtime.block_on(future.compat())
}

#[fbinit::test]
fn test_counter_simple(fb: FacebookInit) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let ctx = CoreContext::test_mock(fb);
    let mutable_counters = create_db();
    run_future(
        &runtime,
        mutable_counters.set_counter(ctx.clone(), REPO_ZERO, &"counter".to_string(), 1, None),
    )
    .unwrap();
    assert_eq!(
        run_future(
            &runtime,
            mutable_counters.get_counter(ctx.clone(), REPO_ZERO, &"counter".to_string()),
        )
        .unwrap(),
        Some(1)
    );

    run_future(
        &runtime,
        mutable_counters.set_counter(ctx.clone(), REPO_ZERO, &"counter".to_string(), 2, None),
    )
    .unwrap();
    assert_eq!(
        run_future(
            &runtime,
            mutable_counters.get_counter(ctx.clone(), REPO_ZERO, &"counter".to_string()),
        )
        .unwrap(),
        Some(2)
    );

    // Update counter from another repo
    run_future(
        &runtime,
        mutable_counters.set_counter(ctx.clone(), REPO_ONE, &"counter".to_string(), 3, None),
    )
    .unwrap();
    assert_eq!(
        run_future(
            &runtime,
            mutable_counters.get_counter(ctx.clone(), REPO_ONE, &"counter".to_string()),
        )
        .unwrap(),
        Some(3)
    );
    assert_eq!(
        run_future(
            &runtime,
            mutable_counters.get_counter(ctx.clone(), REPO_ZERO, &"counter".to_string()),
        )
        .unwrap(),
        Some(2)
    );
}

#[fbinit::test]
fn test_counter_conditional_update(fb: FacebookInit) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let ctx = CoreContext::test_mock(fb);
    let mutable_counters = create_db();

    let counter = "counter".to_string();

    run_future(
        &runtime,
        mutable_counters.set_counter(ctx.clone(), REPO_ZERO, &counter, 1, None),
    )
    .unwrap();
    assert_eq!(
        run_future(
            &runtime,
            mutable_counters.get_counter(ctx.clone(), REPO_ZERO, &counter),
        )
        .unwrap(),
        Some(1)
    );

    run_future(
        &runtime,
        mutable_counters.set_counter(ctx.clone(), REPO_ZERO, &counter, 2, Some(1)),
    )
    .unwrap();
    assert_eq!(
        run_future(
            &runtime,
            mutable_counters.get_counter(ctx.clone(), REPO_ZERO, &counter),
        )
        .unwrap(),
        Some(2)
    );

    // Wasn't updated because prev_value is incorrect
    assert!(
        !run_future(
            &runtime,
            mutable_counters.set_counter(ctx.clone(), REPO_ZERO, &counter, 3, Some(1)),
        )
        .unwrap()
    );
    assert_eq!(
        run_future(
            &runtime,
            mutable_counters.get_counter(ctx.clone(), REPO_ZERO, &counter),
        )
        .unwrap(),
        Some(2)
    );

    // Trying to update another counter, make sure it doesn't touch first counter
    let another_counter_name = "counter2".to_string();
    assert!(
        !run_future(
            &runtime,
            mutable_counters.set_counter(ctx.clone(), REPO_ZERO, &another_counter_name, 3, Some(2)),
        )
        .unwrap()
    );
    assert_eq!(
        run_future(
            &runtime,
            mutable_counters.get_counter(ctx.clone(), REPO_ZERO, &counter),
        )
        .unwrap(),
        Some(2)
    );
    assert_eq!(
        run_future(
            &runtime,
            mutable_counters.get_counter(ctx.clone(), REPO_ZERO, &another_counter_name),
        )
        .unwrap(),
        None
    );
}
