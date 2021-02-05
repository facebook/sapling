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
use limits::types::{MononokeThrottleLimit, MononokeThrottleLimits, RateLimits};
use permission_checker::{MononokeIdentitySet, MononokeIdentitySetExt};
pub use session_id::SessionId;
use std::{fmt, sync::Arc, time::Duration};

#[cfg(fbcode_build)]
mod facebook;
#[cfg(fbcode_build)]
use facebook as impl_mod;

#[cfg(not(fbcode_build))]
mod oss;
#[cfg(not(fbcode_build))]
use oss as impl_mod;

pub type ArcLoadLimiter = Arc<dyn LoadLimiter + Send + Sync + 'static>;
pub type BoxLoadLimiter = Box<dyn LoadLimiter + Send + Sync + 'static>;

pub type LoadCost = f64;

#[derive(Debug)]
pub enum Metric {
    EgressBytes,
    IngressBlobstoreBytes,
    EgressTotalManifests,
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

#[derive(Clone)]
pub struct LoadLimiterEnvironment {
    fb: FacebookInit,
    category: Arc<String>,
    handle: ConfigHandle<MononokeThrottleLimits>,
}

impl LoadLimiterEnvironment {
    pub fn new(
        fb: FacebookInit,
        category: String,
        handle: ConfigHandle<MononokeThrottleLimits>,
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
            impl_mod::select_region_capacity(&config.datacenter_prefix_capacity).unwrap_or(100.0);

        let hostprefix = hostname.map(|h| extract_hostprefix(h));

        let hostprefix_config = hostprefix
            .and_then(|hostprefix| config.hostprefixes.get(hostprefix))
            .unwrap_or(&config.defaults);

        let multiplier = if identities.is_quicksand() {
            region_percentage / 100.0 * config.quicksand_multiplier
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

        impl_mod::build_load_limiter(
            self.fb,
            throttle_limits,
            config.rate_limits.clone(),
            self.category.as_ref().clone(),
        )
    }
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
