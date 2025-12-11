/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;

use crate::BoxGlobalTimeWindowCounter;
use crate::GlobalTimeWindowCounter;
use crate::GlobalTimeWindowCounterBuilder;

struct AlwaysZeroCounter {}

#[async_trait]
impl GlobalTimeWindowCounter for AlwaysZeroCounter {
    async fn get(&self, _time_window: u32) -> Result<f64> {
        Ok(0.0)
    }

    fn bump(&self, _value: f64) {}
}

impl GlobalTimeWindowCounterBuilder {
    pub fn build(
        _fb: FacebookInit,
        _category: impl AsRef<str>,
        _key: impl AsRef<str>,
        _min_time_window: u32,
        _max_time_window: u32,
    ) -> BoxGlobalTimeWindowCounter {
        Box::new(AlwaysZeroCounter {})
    }
}
