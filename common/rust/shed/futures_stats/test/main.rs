/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use futures::FutureExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream;
use futures_stats::TimedFutureExt;
use futures_stats::TimedStreamExt;

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fut = future::lazy(|_| {
        println!("future polled");
        Ok::<(), ()>(())
    })
    .timed()
    .map(|(stats, res)| {
        println!("{:#?}", stats);
        res
    });
    runtime.block_on(fut).unwrap();

    let stream = stream::iter([1, 2, 3].map(Ok::<u32, ()>)).timed(|stats| {
        println!("{:#?}", stats);
    });
    runtime
        .block_on(stream.try_for_each(|_| future::ok(())))
        .unwrap();

    let empty: Vec<Result<u32, ()>> = vec![];
    let stream = stream::iter(empty).timed(|stats| {
        assert!(stats.first_item_time.is_none());
    });
    runtime
        .block_on(stream.try_for_each(|_| future::ok(())))
        .unwrap();
}
