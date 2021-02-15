/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Result;
use async_trait::async_trait;
use cached_config::ConfigHandle;
use fbinit::FacebookInit;
use limits::types::{MononokeThrottleLimit, RateLimits};
use permission_checker::{MononokeIdentitySet, MononokeIdentitySetExt};
pub use session_id::SessionId;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::{fmt, sync::Arc, time::Duration};
use thiserror::Error;

#[cfg(fbcode_build)]
mod facebook;
#[cfg(fbcode_build)]
use facebook as impl_mod;

#[cfg(not(fbcode_build))]
mod oss;
#[cfg(not(fbcode_build))]
use oss as impl_mod;

pub mod config;
use config::{MononokeThrottleLimitsConfig, StaticSlicedLimitsConfig};

pub type ArcLoadLimiter = Arc<dyn LoadLimiter + Send + Sync + 'static>;
pub type BoxLoadLimiter = Box<dyn LoadLimiter + Send + Sync + 'static>;

pub type LoadCost = f64;

#[derive(Debug, Copy, Clone)]
pub enum Metric {
    EgressBytes,
    IngressBlobstoreBytes,
    EgressTotalManifests,
    EgressGetpackFiles,
    EgressCommits,
}

#[must_use]
#[derive(Debug, Error)]
pub enum ThrottleReason {
    #[error("In throttled slice")]
    ThrottledSlice,
    #[error("Throttled by {:?} over {:?}", .0, .1)]
    ThrottledMetric(Metric, Duration),
}

#[async_trait]
pub trait LoadLimiter: fmt::Debug {
    async fn check_throttle(
        &self,
        metric: Metric,
        window: Duration,
    ) -> Result<Result<(), ThrottleReason>>;

    fn bump_load(&self, metric: Metric, load: LoadCost);

    fn category(&self) -> &str;

    fn rate_limits(&self) -> &RateLimits;
}

#[derive(Clone)]
pub struct LoadLimiterEnvironment {
    fb: FacebookInit,
    category: Arc<String>,
    handle: ConfigHandle<MononokeThrottleLimitsConfig>,
}

impl LoadLimiterEnvironment {
    pub fn new(
        fb: FacebookInit,
        category: String,
        handle: ConfigHandle<MononokeThrottleLimitsConfig>,
    ) -> Self {
        Self {
            fb,
            category: Arc::new(category),
            handle,
        }
    }

    pub fn get(&self, identities: &MononokeIdentitySet, hostname: Option<&str>) -> BoxLoadLimiter {
        let config = self.handle.get();

        let region_percentage =
            impl_mod::select_region_capacity(&config.raw_config.datacenter_prefix_capacity)
                .unwrap_or(100.0);

        let hostprefix = identities
            .hostprefix()
            .or_else(|| Some(extract_hostprefix(hostname?)));

        let hostprefix_config = hostprefix
            .and_then(|hostprefix| config.raw_config.hostprefixes.get(hostprefix))
            .unwrap_or(&config.raw_config.defaults);

        let multiplier = if identities.is_quicksand() {
            region_percentage / 100.0 * config.raw_config.quicksand_multiplier
        } else {
            region_percentage / 100.0
        };

        let throttle_limits = MononokeThrottleLimit {
            egress_bytes: hostprefix_config.egress_bytes * multiplier,
            ingress_blobstore_bytes: hostprefix_config.ingress_blobstore_bytes * multiplier,
            total_manifests: hostprefix_config.total_manifests * multiplier,
            quicksand_manifests: hostprefix_config.quicksand_manifests * multiplier,
            getfiles_files: hostprefix_config.getfiles_files * multiplier,
            getpack_files: hostprefix_config.getpack_files * multiplier,
            commits: hostprefix_config.commits * multiplier,
        };


        let in_throttled_slice = if let Some(ssl) = config.static_sliced_limits.as_ref() {
            is_client_in_throttled_slice(identities, ssl)
        } else {
            false
        };

        impl_mod::build_load_limiter(
            self.fb,
            throttle_limits,
            config.raw_config.rate_limits.clone(),
            self.category.as_ref().clone(),
            in_throttled_slice,
        )
    }
}

pub(crate) fn is_client_in_throttled_slice(
    identities: &MononokeIdentitySet,
    static_sliced_limits: &StaticSlicedLimitsConfig,
) -> bool {
    let hostname = if let Some(hostname) = identities.hostname() {
        hostname
    } else {
        return false;
    };

    let limits = &static_sliced_limits.limits;
    let nonce = &static_sliced_limits.nonce;
    for limit in limits.iter() {
        let pct_limit = limit.limit_pct;
        if identities.is_superset(&limit.identities) {
            let mut hasher = DefaultHasher::new();
            hostname.hash(&mut hasher);
            nonce.hash(&mut hasher);
            if hasher.finish() % 100 < pct_limit {
                return true;
            }
        }
    }
    false
}

fn extract_hostprefix(hostname: &str) -> &str {
    let index = hostname.find(|c: char| !c.is_ascii_alphabetic());
    match index {
        Some(index) => hostname.split_at(index).0,
        None => hostname,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_hostname_scheme() {
        assert_eq!(extract_hostprefix("devvm001.lla1.facebook.com"), "devvm");
        assert_eq!(extract_hostprefix("hg001.lla1.facebook.com"), "hg");
        assert_eq!(extract_hostprefix("ololo"), "ololo");
        assert_eq!(extract_hostprefix(""), "");
    }
}
