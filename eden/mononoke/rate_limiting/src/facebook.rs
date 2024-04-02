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
use fbwhoami::FbWhoAmI;
use permission_checker::MononokeIdentitySet;
use ratelim::loadlimiter;
use ratelim::loadlimiter::LoadCost;
use ratelim::loadlimiter::LoadLimitCounter;

use crate::BoxRateLimiter;
use crate::Metric;
use crate::MononokeRateLimitConfig;
use crate::RateLimitBody;
use crate::RateLimitReason;
use crate::RateLimiter;

pub fn get_region_capacity(datacenter_capacity: &BTreeMap<String, i32>) -> Option<i32> {
    let whoami = FbWhoAmI::get().expect("Failed to get fbwhoami information");

    datacenter_capacity
        .get(whoami.region_datacenter_prefix.as_ref()?)
        .copied()
}

pub fn create_rate_limiter(
    fb: FacebookInit,
    category: String,
    config: Arc<MononokeRateLimitConfig>,
) -> BoxRateLimiter {
    Box::new(MononokeRateLimits {
        config,
        fb,
        category: category.clone(),
        load_limits: Arc::new(LoadLimitsInner::new(category)),
    })
}

#[async_trait]
impl RateLimiter for MononokeRateLimits {
    async fn check_rate_limit(
        &self,
        metric: Metric,
        identities: &MononokeIdentitySet,
    ) -> Result<Result<(), RateLimitReason>, Error> {
        for limit in &self.config.rate_limits {
            if limit.metric != metric {
                continue;
            }

            if !limit.applies_to_client(identities) {
                continue;
            }

            if loadlimiter::should_throttle(
                self.fb,
                self.counter(metric),
                limit.body.raw_config.limit * self.config.region_weight,
                limit.body.window,
            )
            .await?
            {
                return Ok(Err(RateLimitReason::RateLimitedMetric(
                    metric,
                    limit.body.window,
                )));
            }
        }

        Ok(Ok(()))
    }

    fn check_load_shed(&self, identities: &MononokeIdentitySet) -> Result<(), RateLimitReason> {
        for limit in &self.config.load_shed_limits {
            limit.should_load_shed(self.fb, Some(identities))?;
        }

        Ok(())
    }

    fn bump_load(&self, metric: Metric, load: LoadCost) {
        loadlimiter::bump_load(self.fb, self.counter(metric), load)
    }

    fn category(&self) -> &str {
        &self.category
    }

    fn commits_per_author_limit(&self) -> Option<RateLimitBody> {
        Some(self.config.commits_per_author.clone())
    }

    fn total_file_changes_limit(&self) -> Option<RateLimitBody> {
        self.config.total_file_changes.clone()
    }
}

#[derive(Clone)]
pub struct MononokeRateLimits {
    config: Arc<MononokeRateLimitConfig>,
    fb: FacebookInit,
    category: String,
    load_limits: Arc<LoadLimitsInner>,
}

impl std::fmt::Debug for MononokeRateLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("MononokeRateLimits")
            .field("category", &self.category)
            .field("region_weight", &self.config.region_weight)
            .field("load_limits", &self.load_limits)
            .finish()
    }
}

#[derive(Debug)]
struct LoadLimitsInner {
    egress_bytes: LoadLimitCounter,
    total_manifests: LoadLimitCounter,
    getpack_files: LoadLimitCounter,
    commits: LoadLimitCounter,
}

impl LoadLimitsInner {
    pub fn new(category: String) -> Self {
        Self {
            egress_bytes: LoadLimitCounter {
                category: category.clone(),
                key: make_regional_limit_key("egress-bytes"),
            },
            total_manifests: LoadLimitCounter {
                category: category.clone(),
                key: make_regional_limit_key("egress-total-manifests"),
            },
            getpack_files: LoadLimitCounter {
                category: category.clone(),
                key: make_regional_limit_key("egress-getpack-files"),
            },
            commits: LoadLimitCounter {
                category,
                key: make_regional_limit_key("egress-commits"),
            },
        }
    }
}

fn make_regional_limit_key(prefix: &str) -> String {
    let fbwhoami = FbWhoAmI::get().unwrap();
    let region = fbwhoami.region_datacenter_prefix.as_deref().unwrap();
    let mut key = prefix.to_owned();
    key.push(':');
    key.push_str(region);
    key
}

impl MononokeRateLimits {
    fn counter(&self, metric: Metric) -> &LoadLimitCounter {
        match metric {
            Metric::EgressBytes => &self.load_limits.egress_bytes,
            Metric::TotalManifests => &self.load_limits.total_manifests,
            Metric::GetpackFiles => &self.load_limits.getpack_files,
            Metric::Commits => &self.load_limits.commits,
        }
    }
}
