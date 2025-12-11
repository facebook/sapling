/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;
use ratelim::time_window_counter::TimeWindowCounter as RatelimCounter;

use crate::BoxGlobalTimeWindowCounter;
use crate::GlobalTimeWindowCounter;
use crate::GlobalTimeWindowCounterBuilder;

#[async_trait]
impl GlobalTimeWindowCounter for RatelimCounter {
    async fn get(&self, time_window: u32) -> Result<f64> {
        self.get_future(time_window).await
    }

    fn bump(&self, value: f64) {
        self.bump(value)
    }
}

impl GlobalTimeWindowCounterBuilder {
    pub fn build(
        fb: FacebookInit,
        category: impl AsRef<str>,
        key: impl AsRef<str>,
        min_time_window: u32,
        max_time_window: u32,
    ) -> BoxGlobalTimeWindowCounter {
        Box::new(RatelimCounter::new(
            fb,
            category,
            key,
            min_time_window,
            max_time_window,
        ))
    }
}
