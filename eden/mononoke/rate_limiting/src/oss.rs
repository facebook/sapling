/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Error;
use async_trait::async_trait;
use fbinit::FacebookInit;
use ods_counters::OdsCounterManager;
use permission_checker::MononokeIdentitySet;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::BoxRateLimiter;
use crate::LoadCost;
use crate::LoadShedResult;
use crate::Metric;
use crate::MononokeRateLimitConfig;
#[cfg(fbcode_build)]
use crate::RateLimit;
#[cfg(fbcode_build)]
use crate::RateLimitBody;
use crate::RateLimitResult;
use crate::RateLimiter;
use crate::Scope;

pub fn create_rate_limiter(
    _fb: FacebookInit,
    category: String,
    _config: Arc<MononokeRateLimitConfig>,
    _counter_manager: Arc<RwLock<OdsCounterManager>>,
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
        _main_id: Option<&str>,
        _scuba: &mut MononokeScubaSampleBuilder,
        _atlas: Option<bool>,
    ) -> Result<RateLimitResult, Error> {
        Ok(RateLimitResult::Pass)
    }

    fn check_load_shed(
        &self,
        _identities: &MononokeIdentitySet,
        _main_id: Option<&str>,
        _scuba: &mut MononokeScubaSampleBuilder,
        _atlas: Option<bool>,
    ) -> LoadShedResult {
        LoadShedResult::Pass
    }

    fn bump_load(&self, _metric: Metric, _scope: Scope, _load: LoadCost) {}

    fn category(&self) -> &str {
        &self.category
    }

    fn find_rate_limit(
        &self,
        _metric: Metric,
        _identities: Option<MononokeIdentitySet>,
        _main_id: Option<&str>,
        _atlas: Option<bool>,
    ) -> Option<crate::RateLimit> {
        None
    }
}
