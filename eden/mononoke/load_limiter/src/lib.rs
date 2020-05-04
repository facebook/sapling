/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

#[cfg(fbcode_build)]
mod facebook;

use anyhow::Result;
use async_trait::async_trait;
use limits::types::RateLimits;
pub use session_id::SessionId;
use std::{fmt, sync::Arc, time::Duration};

pub type ArcLoadLimiter = Arc<dyn LoadLimiter + Send + Sync + 'static>;
pub type BoxLoadLimiter = Box<dyn LoadLimiter + Send + Sync + 'static>;

pub type LoadCost = f64;

#[derive(Debug)]
pub enum Metric {
    EgressBytes,
    IngressBlobstoreBytes,
    EgressTotalManifests,
    EgressGetfilesFiles,
    EgressGetpackFiles,
    EgressCommits,
}

#[async_trait]
pub trait LoadLimiter: fmt::Debug {
    async fn should_throttle(&self, metric: Metric, window: Duration) -> Result<bool>;

    fn bump_load(&self, metric: Metric, load: LoadCost);

    fn category(&self) -> &str;

    fn rate_limits(&self) -> &RateLimits;
}

pub struct LoadLimiterBuilder {}

#[cfg(not(fbcode_build))]
mod r#impl {
    use super::*;

    use fbinit::FacebookInit;
    use limits::types::MononokeThrottleLimit;

    impl LoadLimiterBuilder {
        pub fn build(
            _fb: FacebookInit,
            _throttle_limits: MononokeThrottleLimit,
            rate_limits: RateLimits,
            category: String,
        ) -> BoxLoadLimiter {
            Box::new(NoopLimiter {
                category,
                rate_limits,
            })
        }
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
}
