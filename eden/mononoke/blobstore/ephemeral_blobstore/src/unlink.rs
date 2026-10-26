/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU32;
use std::num::NonZeroUsize;
use std::time::Duration;

use governor::DefaultDirectRateLimiter;
use governor::Jitter;
use governor::Quota;
use governor::RateLimiter;

const RATE_LIMIT_JITTER: Duration = Duration::from_millis(5);

const RATE_LIMIT_BURST_DIVISOR: u32 = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnlinkPolicy {
    pub max_unlinks_per_sec: Option<NonZeroU32>,
    pub retry: Option<UnlinkRetry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnlinkRetry {
    pub max_attempts: NonZeroUsize,
    pub initial_delay: Duration,
    pub max_delay: Duration,
}

pub(crate) struct Unlinker {
    retry: Option<UnlinkRetry>,
    rate_limiter: Option<DefaultDirectRateLimiter>,
}

impl Unlinker {
    pub(crate) fn new(policy: UnlinkPolicy) -> Self {
        let rate_limiter = policy.max_unlinks_per_sec.map(|rate| {
            let burst =
                NonZeroU32::new(rate.get() / RATE_LIMIT_BURST_DIVISOR).unwrap_or(NonZeroU32::MIN);
            RateLimiter::direct(Quota::per_second(rate).allow_burst(burst))
        });
        Self {
            retry: policy.retry,
            rate_limiter,
        }
    }

    pub(crate) fn retry(&self) -> Option<UnlinkRetry> {
        self.retry
    }

    pub(crate) async fn until_ready(&self) {
        if let Some(rate_limiter) = &self.rate_limiter {
            rate_limiter
                .until_ready_with_jitter(Jitter::up_to(RATE_LIMIT_JITTER))
                .await;
        }
    }
}
