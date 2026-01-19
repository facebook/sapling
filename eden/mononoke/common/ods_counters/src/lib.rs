/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This library is used to query ODS counters
//! It should not be used for counters that are available locally
//! Those should be queried from the local host via fb303
use async_trait::async_trait;

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;

#[cfg(fbcode_build)]
pub use facebook::OdsCounterManager;
#[cfg(fbcode_build)]
pub use facebook::periodic_fetch_counter;
#[cfg(not(fbcode_build))]
pub use oss::OdsCounterManager;
#[cfg(not(fbcode_build))]
pub use oss::periodic_fetch_counter;

#[async_trait]
pub trait CounterManager {
    fn add_counter(
        &mut self,
        entity: String,
        key: String,
        reduce: Option<String>,
        transform: Option<String>,
    );

    fn get_counter_value(
        &self,
        entity: &str,
        key: &str,
        reduce: Option<&str>,
        transform: Option<&str>,
    ) -> Option<f64>;
}
