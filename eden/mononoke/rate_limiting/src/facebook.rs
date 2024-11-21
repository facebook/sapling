/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use fbinit::FacebookInit;
use fbwhoami::FbWhoAmI;
use permission_checker::MononokeIdentitySet;
use rate_limiting_config::RateLimitStatus;
use ratelim::loadlimiter;
use ratelim::loadlimiter::LoadCost;
use ratelim::loadlimiter::LoadLimitCounter;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::BoxRateLimiter;
use crate::LoadShedResult;
use crate::Metric;
use crate::MononokeRateLimitConfig;
use crate::RateLimitBody;
use crate::RateLimitReason;
use crate::RateLimitResult;
use crate::RateLimiter;
use crate::Scope;

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

pub fn log_or_enforce_status(
    body: &RateLimitBody,
    metric: Metric,
    scuba: &mut MononokeScubaSampleBuilder,
) -> RateLimitResult {
    match body.raw_config.status {
        RateLimitStatus::Disabled => RateLimitResult::Pass,
        RateLimitStatus::Tracked => {
            scuba.log_with_msg(
                "Would have rate limited",
                format!(
                    "{:?}",
                    (RateLimitReason::RateLimitedMetric(metric, body.window))
                ),
            );
            RateLimitResult::Pass
        }
        RateLimitStatus::Enforced => {
            RateLimitResult::Fail(RateLimitReason::RateLimitedMetric(metric, body.window))
        }
        _ => panic!(
            "Thrift enums aren't real enums once in Rust. We have to account for other values here."
        ),
    }
}

#[async_trait]
impl RateLimiter for MononokeRateLimits {
    async fn check_rate_limit(
        &self,
        metric: Metric,
        identities: &MononokeIdentitySet,
        main_id: Option<&str>,
        scuba: &mut MononokeScubaSampleBuilder,
    ) -> Result<RateLimitResult, Error> {
        for limit in &self.config.rate_limits {
            let fci_metric = limit.fci_metric;

            if fci_metric.metric != metric {
                continue;
            }

            if !limit.applies_to_client(identities, main_id) {
                continue;
            }

            if loadlimiter::should_throttle(
                self.fb,
                self.counter(fci_metric.metric, fci_metric.scope),
                limit.body.raw_config.limit,
                limit.fci_metric.window,
            )
            .await?
            {
                match log_or_enforce_status(&limit.body, fci_metric.metric, scuba) {
                    RateLimitResult::Pass => {
                        break;
                    }
                    RateLimitResult::Fail(reason) => RateLimitResult::Fail(reason),
                };
            }
        }
        Ok(RateLimitResult::Pass)
    }

    fn check_load_shed(
        &self,
        identities: &MononokeIdentitySet,
        main_id: Option<&str>,
        scuba: &mut MononokeScubaSampleBuilder,
    ) -> LoadShedResult {
        for limit in &self.config.load_shed_limits {
            if let LoadShedResult::Fail(reason) =
                limit.should_load_shed(self.fb, Some(identities), main_id, scuba)
            {
                return LoadShedResult::Fail(reason);
            }
        }
        LoadShedResult::Pass
    }

    fn bump_load(&self, metric: Metric, scope: Scope, load: LoadCost) {
        loadlimiter::bump_load(self.fb, self.counter(metric, scope), load)
    }

    fn category(&self) -> &str {
        &self.category
    }

    fn commits_per_author_limit(&self) -> Option<crate::RateLimit> {
        self.config
            .rate_limits
            .iter()
            .find(|r| r.fci_metric.metric == Metric::CommitsPerAuthor)
            .cloned()
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
            .field("load_limits", &self.load_limits)
            .finish()
    }
}

#[derive(Debug)]
struct LoadLimitsInner {
    regional_egress_bytes: LoadLimitCounter,
    regional_total_manifests: LoadLimitCounter,
    regional_getpack_files: LoadLimitCounter,
    regional_commits: LoadLimitCounter,
    commits_per_author: LoadLimitCounter,
}

impl LoadLimitsInner {
    pub fn new(category: String) -> Self {
        Self {
            regional_egress_bytes: LoadLimitCounter {
                category: category.clone(),
                key: make_regional_limit_key("egress-bytes"),
            },
            regional_total_manifests: LoadLimitCounter {
                category: category.clone(),
                key: make_regional_limit_key("egress-total-manifests"),
            },
            regional_getpack_files: LoadLimitCounter {
                category: category.clone(),
                key: make_regional_limit_key("egress-getpack-files"),
            },
            regional_commits: LoadLimitCounter {
                category: category.clone(),
                key: make_regional_limit_key("egress-commits"),
            },
            commits_per_author: LoadLimitCounter {
                category,
                key: "commits_per_author".to_string(),
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
    fn counter(&self, metric: Metric, scope: Scope) -> &LoadLimitCounter {
        match (metric, scope) {
            (Metric::EgressBytes, Scope::Regional) => &self.load_limits.regional_egress_bytes,
            (Metric::TotalManifests, Scope::Regional) => &self.load_limits.regional_total_manifests,
            (Metric::GetpackFiles, Scope::Regional) => &self.load_limits.regional_getpack_files,
            (Metric::Commits, Scope::Regional) => &self.load_limits.regional_commits,
            (Metric::CommitsPerAuthor, Scope::Global) => &self.load_limits.commits_per_author,
            _ => panic!("Unsupported metric/scope combination"),
        }
    }
}
