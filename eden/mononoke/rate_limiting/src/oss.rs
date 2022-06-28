/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use fbinit::FacebookInit;
use permission_checker::MononokeIdentitySet;

use crate::BoxRateLimiter;
use crate::LoadCost;
use crate::Metric;
use crate::MononokeRateLimitConfig;
use crate::RateLimitBody;
use crate::RateLimitReason;
use crate::RateLimiter;

pub fn get_region_capacity(_datacenter_capacity: &BTreeMap<String, i32>) -> Option<i32> {
    None
}

pub fn create_rate_limiter(
    _fb: FacebookInit,
    category: String,
    _config: Arc<MononokeRateLimitConfig>,
) -> BoxRateLimiter {
    Box::new(FakeLimiter { category })
}

#[derive(Debug)]
struct FakeLimiter {
    category: String,
}

#[async_trait]
impl RateLimiter for FakeLimiter {
    async fn check_rate_limit(
        &self,
        _metric: Metric,
        _identities: &MononokeIdentitySet,
    ) -> Result<Result<(), RateLimitReason>, Error> {
        Ok(Ok(()))
    }

    fn check_load_shed(&self, _identities: &MononokeIdentitySet) -> Result<(), RateLimitReason> {
        Ok(())
    }

    fn bump_load(&self, _metric: Metric, _load: LoadCost) {}

    fn category(&self) -> &str {
        &self.category
    }

    fn commits_per_author_limit(&self) -> Option<RateLimitBody> {
        None
    }

    fn total_file_changes_limit(&self) -> Option<RateLimitBody> {
        None
    }
}
