/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use serde::de::Deserializer;
use serde::de::Error as _;
use serde::Deserialize;

use crate::LoadShedLimit;
use crate::Metric;
use crate::MononokeRateLimitConfig;
use crate::RateLimit;
use crate::RateLimitBody;
use crate::StaticSlice;
use crate::Target;

#[cfg(fbcode_build)]
pub use crate::facebook::get_region_capacity;
#[cfg(not(fbcode_build))]
pub use crate::oss::get_region_capacity;

impl TryFrom<rate_limiting_config::Target> for Target {
    type Error = Error;

    fn try_from(value: rate_limiting_config::Target) -> Result<Self, Self::Error> {
        match value {
            rate_limiting_config::Target::not_target(t) => {
                Ok(Target::NotTarget(Box::new((*t).try_into()?)))
            }
            rate_limiting_config::Target::and_target(t) => Ok(Target::AndTarget(
                t.into_iter()
                    .map(|t| t.try_into())
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            rate_limiting_config::Target::or_target(t) => Ok(Target::OrTarget(
                t.into_iter()
                    .map(|t| t.try_into())
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            rate_limiting_config::Target::identity(i) => {
                Ok(Target::Identity(FromStr::from_str(&i)?))
            }
            rate_limiting_config::Target::static_slice(s) => {
                let slice_pct = s.slice_pct.try_into()?;
                Ok(Target::StaticSlice(StaticSlice {
                    slice_pct,
                    nonce: s.nonce,
                }))
            }
            _ => Err(anyhow!("Invalid target")),
        }
    }
}

impl TryFrom<rate_limiting_config::RateLimitBody> for RateLimitBody {
    type Error = Error;

    fn try_from(value: rate_limiting_config::RateLimitBody) -> Result<Self, Self::Error> {
        let window: u64 = value.window.try_into().context("Invalid window")?;

        Ok(Self {
            raw_config: value,
            window: Duration::from_secs(window),
        })
    }
}

impl TryFrom<rate_limiting_config::RegionalMetric> for Metric {
    type Error = Error;

    fn try_from(value: rate_limiting_config::RegionalMetric) -> Result<Self, Self::Error> {
        match value {
            rate_limiting_config::RegionalMetric::EgressBytes => Ok(Metric::EgressBytes),
            rate_limiting_config::RegionalMetric::TotalManifests => Ok(Metric::TotalManifests),
            rate_limiting_config::RegionalMetric::GetpackFiles => Ok(Metric::GetpackFiles),
            rate_limiting_config::RegionalMetric::Commits => Ok(Metric::Commits),
            _ => Err(anyhow!("Invalid RegionalMetric")),
        }
    }
}

impl TryFrom<rate_limiting_config::RateLimit> for RateLimit {
    type Error = Error;

    fn try_from(value: rate_limiting_config::RateLimit) -> Result<Self, Self::Error> {
        let body = value
            .limit
            .clone()
            .try_into()
            .context("Invalid limit body")?;

        let target = value
            .target
            .clone()
            .map(Target::try_from)
            .transpose()
            .context("Invalid target")?;

        let metric = value.metric.clone().try_into().context("Invalid metric")?;

        Ok(Self {
            body,
            metric,
            target,
        })
    }
}

impl TryFrom<rate_limiting_config::LoadShedLimit> for LoadShedLimit {
    type Error = Error;

    fn try_from(value: rate_limiting_config::LoadShedLimit) -> Result<Self, Self::Error> {
        let target = value
            .target
            .clone()
            .map(Target::try_from)
            .transpose()
            .context("Invalid target")?;

        Ok(Self {
            raw_config: value,
            target,
        })
    }
}

impl<'de> Deserialize<'de> for LoadShedLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = rate_limiting_config::LoadShedLimit::deserialize(deserializer)?;
        let load_shed_limit = raw
            .try_into()
            .map_err(|e| D::Error::custom(format!("{:?}", e)))?;

        Ok(load_shed_limit)
    }
}

impl<'de> Deserialize<'de> for MononokeRateLimitConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw_config = rate_limiting_config::MononokeRateLimits::deserialize(deserializer)?;

        let dc_prefix_capacity = &raw_config.datacenter_prefix_capacity;

        // We scale the limits used for RegionalMetrics according to the amount of capacity in a
        // region. Calculate the fraction of tasks that this region accounts for.
        let region_weight = match get_region_capacity(dc_prefix_capacity) {
            Some(capacity) => {
                capacity as f64 / dc_prefix_capacity.values().map(|c| *c as f64).sum::<f64>()
            }
            None => 1.0,
        };

        let rate_limits = raw_config
            .rate_limits
            .clone()
            .into_iter()
            .map(|r| r.try_into())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| D::Error::custom(format!("{:?}", e)))?;

        let load_shed_limits = raw_config
            .load_shed_limits
            .clone()
            .into_iter()
            .map(|r| r.try_into())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| D::Error::custom(format!("{:?}", e)))?;

        let commits_per_author = raw_config
            .commits_per_author
            .clone()
            .try_into()
            .map_err(|e| D::Error::custom(format!("{:?}", e)))?;

        let total_file_changes = raw_config
            .total_file_changes
            .clone()
            .map(|v| v.try_into())
            .transpose()
            .map_err(|e| D::Error::custom(format!("{:?}", e)))?;

        Ok(Self {
            region_weight,
            rate_limits,
            load_shed_limits,
            commits_per_author,
            total_file_changes,
        })
    }
}
