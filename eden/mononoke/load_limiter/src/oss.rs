/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;
use limits::types::{MononokeThrottleLimit, RateLimits};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::{BoxLoadLimiter, LoadCost, LoadLimiter, Metric};

pub fn select_region_capacity(_: &BTreeMap<String, f64>) -> Option<f64> {
    None
}

pub fn build_load_limiter(
    _: FacebookInit,
    _: MononokeThrottleLimit,
    rate_limits: RateLimits,
    category: String,
) -> BoxLoadLimiter {
    Box::new(NoopLimiter {
        category,
        rate_limits,
    })
}

#[derive(Debug)]
struct NoopLimiter {
    category: String,
    rate_limits: RateLimits,
}

#[async_trait]
impl LoadLimiter for NoopLimiter {
    async fn should_throttle(&self, _metric: Metric, _window: Duration) -> Result<bool> {
        Ok(false)
    }

    fn bump_load(&self, _metric: Metric, _load: LoadCost) {}

    fn category(&self) -> &str {
        &self.category
    }

    fn rate_limits(&self) -> &RateLimits {
        &self.rate_limits
    }
}
