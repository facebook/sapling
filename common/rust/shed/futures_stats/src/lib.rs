/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

//! An extension to types implementing the `Future` trait that adds a `timed()` method.
//! This method returns a Future that times the execution of the wrapped future, and
//! passes this value to a callback upon completion of the Future. This is useful for
//! recording performance information about Futures.

#![deny(warnings, missing_docs, clippy::all, rustdoc::broken_intra_doc_links)]

use std::time::Duration;

pub mod futures03;

// Export new Futures 0.3 API, which has different names.
pub use futures03::TimedFutureExt;
pub use futures03::TimedStreamExt;
pub use futures03::TimedTryFutureExt;
pub use futures03::TimedTryStreamExt;

/// A structure that holds some basic statistics for Future.
#[derive(Clone, Debug)]
pub struct FutureStats {
    /// Time elapsed between the first time the Future was polled until it completed.
    pub completion_time: Duration,

    /// Cumulative time the wrapped Future spent in its `poll()` function. This should
    /// usually be small -- large amounts of time spent in `poll()` may indicate that the
    /// Future is spending time performing expensive synchronous work.
    pub poll_time: Duration,

    /// Max time the wrapped Future spent in its `poll()` function.  usually be
    /// small -- large amounts of time spent in `poll()` may indicate that the
    /// Future is blocking event loop with synchronous work.
    pub max_poll_time: Duration,

    /// Number of times that the Future was polled.
    pub poll_count: u64,
}

/// A structure that holds some basic statistics for Stream.
#[derive(Clone, Debug)]
pub struct StreamStats {
    /// Time elapsed between the first time the Stream was polled until it completed.  None if the
    /// Stream was never polled.
    pub completion_time: Option<Duration>,

    /// Time elapsed between the first time the Stream was polled until the first item became available.
    /// None if the Stream did not yield any items.
    pub first_item_time: Option<Duration>,

    /// Cumulative time the wrapped Stream spent in its `poll()` function. This should
    /// usually be small -- large amounts of time spent in `poll()` may indicate that the
    /// Stream is spending time performing expensive synchronous work.
    pub poll_time: Duration,

    /// Max time the wrapped Future spent in its `poll()` function.  usually be
    /// small -- large amounts of time spent in `poll()` may indicate that the
    /// Future is blocking event loop with synchronous work.
    pub max_poll_time: Duration,

    /// Number of times that the Stream was polled.
    pub poll_count: u64,

    /// Number of items in the stream.
    pub count: usize,

    /// Whether the stream was polled to completion.
    pub completed: bool,
}

/// A structure that holds some basic statistics for Stream.
#[derive(Clone, Debug)]
pub struct TryStreamStats {
    /// All the stats that are not try-stream specific.
    pub stream_stats: StreamStats,

    /// Number of errors in the stream.
    pub error_count: usize,

    /// Number of elements in the stream that were emitted before first error
    pub first_error_position: Option<usize>,
}
