/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::time::Duration;

use fbinit::FacebookInit;
use stats_traits::stat_types::BoxHistogram;
use stats_traits::stat_types::BoxLocalCounter;
use stats_traits::stat_types::BoxLocalHistogram;
use stats_traits::stat_types::BoxLocalTimeseries;
use stats_traits::stat_types::Counter;
use stats_traits::stat_types::Histogram;
use stats_traits::stat_types::SingletonCounter;
use stats_traits::stat_types::Timeseries;
use stats_traits::stats_manager::AggregationType;
use stats_traits::stats_manager::BoxStatsManager;
use stats_traits::stats_manager::BucketConfig;
use stats_traits::stats_manager::StatsManager;
use stats_traits::stats_manager::StatsManagerFactory;

pub struct NoopStatsFactory;

impl StatsManagerFactory for NoopStatsFactory {
    fn create(&self) -> BoxStatsManager {
        Box::new(Noop)
    }
}

pub struct Noop;

impl StatsManager for Noop {
    fn aggregate(&self) {}

    fn create_counter(&self, _name: &str) -> BoxLocalCounter {
        Box::new(Noop)
    }

    fn create_timeseries(
        &self,
        _name: &str,
        _aggregation_types: &[AggregationType],
        _intervals: &[Duration],
    ) -> BoxLocalTimeseries {
        Box::new(Noop)
    }

    fn create_histogram(
        &self,
        _name: &str,
        _aggregation_types: &[AggregationType],
        _conf: BucketConfig,
        _percentiles: &[u8],
    ) -> BoxLocalHistogram {
        Box::new(Noop)
    }

    fn create_quantile_stat(
        &self,
        _name: &str,
        _aggregation_types: &[AggregationType],
        _percentiles: &[f32],
        _intervals: &[Duration],
    ) -> BoxHistogram {
        Box::new(Noop)
    }
}

impl Counter for Noop {
    fn increment_value(&self, _value: i64) {}
}

impl Timeseries for Noop {
    fn add_value(&self, _value: i64) {}
    fn add_value_aggregated(&self, _value: i64, _nsamples: u32) {}
}

impl Histogram for Noop {
    fn add_value(&self, _value: i64) {}
    fn add_repeated_value(&self, _value: i64, _nsamples: u32) {}
}

impl SingletonCounter for Noop {
    fn set_value(&self, _fb: FacebookInit, _value: i64) {}
    fn increment_value(&self, _fb: FacebookInit, _value: i64) {}
    fn get_value(&self, _fb: FacebookInit) -> Option<i64> {
        None
    }
}
