/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::time::Duration;

use auto_impl::auto_impl;

use crate::stat_types::BoxHistogram;
use crate::stat_types::BoxLocalCounter;
use crate::stat_types::BoxLocalHistogram;
use crate::stat_types::BoxLocalTimeseries;

pub trait StatsManagerFactory {
    fn create(&self) -> BoxStatsManager;
}

pub type BoxStatsManager = Box<dyn StatsManager + Send + Sync>;

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum AggregationType {
    Sum,
    Count,
    Average,
    Rate,
    Percent,
}

pub struct BucketConfig {
    pub width: u32,
    pub min: u32,
    pub max: u32,
}

#[auto_impl(Box)]
pub trait StatsManager {
    /// Function to be called periodically to aggregate all the stats owned by
    /// this manager.
    fn aggregate(&self);

    /// Create a new instance of [BoxLocalCounter] and bind it to self for
    /// aggregation purposes.
    /// Provided name is the name of the counter.
    fn create_counter(&self, name: &str) -> BoxLocalCounter;

    /// Create new instance of [BoxLocalTimeseries] and bind it to self for
    /// aggregation purposes.
    /// Provided name is the name of the timeseries.
    /// [AggregationType] decides which types of aggragation are exported by
    /// the returned timeseries. The actual implementation is free to assume
    /// some defaults if `aggregation_type` is empty.
    /// The `intervals` provides a list of intervals at which data should be
    /// aggregated, the actual implementation is free (and encouraged) to assume
    /// some defaults if `intervals` is empty.
    fn create_timeseries(
        &self,
        name: &str,
        aggregation_types: &[AggregationType],
        intervals: &[Duration],
    ) -> BoxLocalTimeseries;

    /// Create new instance of [BoxLocalHistogram] and bind it to self for
    /// aggregation purposes.
    /// Provided name is the name of the histogram.
    /// [BucketConfig] configures the aggregation bucket.
    /// [AggregationType] decides which types of aggregation are exported by
    /// the returned timeseries. The actual implementation is free to assume
    /// some defaults if `aggregation_type` is empty.
    /// The `percentiles` provides a list of percentiles for the aggregation
    /// aggregated, the actual implementation is free (and encouraged to) assume
    /// some defaults if `intervals` is empty.
    fn create_histogram(
        &self,
        name: &str,
        aggregation_types: &[AggregationType],
        conf: BucketConfig,
        percentiles: &[u8],
    ) -> BoxLocalHistogram;

    /// Create new instance of `QuantileStat` and bind it to self for
    /// aggregation purposes.
    /// Provided name is the name of the QuantileStat.
    /// [AggregationType] decides which types of aggregation are exported by
    /// the returned timeseries. The actual implementation is free to assume
    /// some defaults if `aggregation_type` is empty.
    /// The `percentiles` provides a list of percentiles for the aggregation
    /// aggregated, the actual implementation is free (and encouraged to) assume
    /// some defaults if `intervals` is empty.
    /// The `intervals` provides a list of intervals at which data should be
    /// aggregated, the actual implementation is free (and encouraged) to assume
    /// some defaults if `intervals` is empty.
    fn create_quantile_stat(
        &self,
        name: &str,
        aggregation_types: &[AggregationType],
        percentiles: &[f32],
        intervals: &[Duration],
    ) -> BoxHistogram;
}
