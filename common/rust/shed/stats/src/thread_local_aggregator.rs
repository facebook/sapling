/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

//! This module provides the means to define stats that are thread local and to
//! schedule a periodic aggregation process of those stats.
//!
//! The assumption behind this library is that there are much more writes to the
//! stats than there are reads. Because of that it's vital that writes are quick
//! while reads can be a little relaxed, so that recent writes might not be
//! visible in the read until an aggregation is called.
//!
//! Thread local stats help with the speed goal of writes - except for
//! infrequent reads no one is racing with the current thread to access those
//! values. As for periodic aggregation - this is achieved via a future running
//! every second thanks to tokio timer and aggregating every thread local stat.
//! The future must be spawned on tokio in order for the aggregation to work.

use std::fmt;
use std::future::Future as NewFuture;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic;
use std::time::Duration;

use futures::FutureExt as _;
use futures::Stream as NewStream;
use futures::StreamExt as _;
use futures::future::ready;
use perthread::ThreadMap;
use stats_traits::stats_manager::BoxStatsManager;
use stats_traits::stats_manager::StatsManager;

static STATS_SCHEDULED: atomic::AtomicBool = atomic::AtomicBool::new(false);
static STATS_AGGREGATOR: LazyLock<StatsAggregator> =
    LazyLock::new(|| StatsAggregator(Mutex::new(Vec::new())));

type SchedulerPreview = std::pin::Pin<Box<dyn NewFuture<Output = ()> + Send>>;

/// This error is returned to indicate that the stats scheduler was already
/// retrieved before and potentially is already running, but might be retrieved
/// again from this error
pub struct StatsScheduledErrorPreview(pub SchedulerPreview);

impl fmt::Debug for StatsScheduledErrorPreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Stats aggregation was already scheduled")
    }
}

impl fmt::Display for StatsScheduledErrorPreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Stats aggregation was already scheduled")
    }
}

impl ::std::error::Error for StatsScheduledErrorPreview {}

struct StatsAggregator(Mutex<Vec<Arc<ThreadMap<BoxStatsManager>>>>);

impl StatsAggregator {
    fn aggregate(&self) {
        let thread_maps = self.0.lock().expect("poisoned mutex");
        for thread_map in &*thread_maps {
            thread_map.for_each(|stats| stats.aggregate());
        }
    }
}

/// Creates the ThreadMap and registers it for periodic calls for aggregation of stats
pub fn create_map() -> Arc<ThreadMap<BoxStatsManager>> {
    let map = ThreadMap::default();
    let map = Arc::new(map);
    let mut vec = STATS_AGGREGATOR.0.lock().expect("poisoned lock");
    vec.push(map.clone());
    map
}

/// Upon the first call to this function it will return a future that results in
/// periodically calling aggregation of stats.
/// On subsequent calls it will return `Error::StatsScheduled` that contain the
/// future, so that the caller might still use it, but knows that it is not the
/// first this function was called.
///
/// # Examples
///
/// ```no_run
/// use stats::schedule_stats_aggregation_preview;
/// use tokio::spawn;
///
/// let s = schedule_stats_aggregation_preview().unwrap();
/// spawn(s);
/// ```
pub fn schedule_stats_aggregation_preview() -> Result<SchedulerPreview, StatsScheduledErrorPreview>
{
    let stream =
        tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(Duration::from_secs(1)));
    let scheduler = schedule_stats_on_stream_preview(stream);

    if STATS_SCHEDULED.swap(true, atomic::Ordering::Relaxed) {
        Err(StatsScheduledErrorPreview(scheduler))
    } else {
        Ok(scheduler)
    }
}

/// Schedules aggregation of stats on the provided stream. This method should not
/// be used directly, it is here for testing purposes
#[doc(hidden)]
pub fn schedule_stats_on_stream_preview<S>(stream: S) -> SchedulerPreview
where
    S: NewStream + Send + 'static,
{
    stream
        .for_each(|_| {
            STATS_AGGREGATOR.aggregate();
            ready(())
        })
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Those tests work on global state so they cannot be run in parallel
    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    #[tokio::test]
    async fn test_schedule_stats_aggregation_preview() {
        let _lock = TEST_MUTEX.lock().expect("poisoned lock");

        if let Err(err) = schedule_stats_aggregation_preview() {
            panic!("Scheduler is not Ok. Reason: {err:?}");
        }

        if schedule_stats_aggregation_preview().is_ok() {
            panic!("Scheduler should already be initialized");
        }

        STATS_SCHEDULED.swap(false, atomic::Ordering::AcqRel);
    }
}
