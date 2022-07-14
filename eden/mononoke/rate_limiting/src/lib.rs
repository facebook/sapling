/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Error;
use async_trait::async_trait;
use cached_config::ConfigHandle;
use fbinit::FacebookInit;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use permission_checker::MononokeIdentitySetExt;
use stats::prelude::*;
use thiserror::Error;

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;

#[cfg(fbcode_build)]
pub use facebook::create_rate_limiter;
#[cfg(fbcode_build)]
pub use facebook::get_region_capacity;
#[cfg(not(fbcode_build))]
pub use oss::create_rate_limiter;
#[cfg(not(fbcode_build))]
pub use oss::get_region_capacity;

pub use rate_limiting_config::RateLimitStatus;

pub mod config;

pub type LoadCost = f64;
pub type BoxRateLimiter = Box<dyn RateLimiter + Send + Sync + 'static>;

#[async_trait]
pub trait RateLimiter {
    async fn check_rate_limit(
        &self,
        metric: Metric,
        identities: &MononokeIdentitySet,
    ) -> Result<Result<(), RateLimitReason>, Error>;

    fn check_load_shed(&self, identities: &MononokeIdentitySet) -> Result<(), RateLimitReason>;

    fn bump_load(&self, metric: Metric, load: LoadCost);

    fn category(&self) -> &str;

    fn commits_per_author_limit(&self) -> Option<RateLimitBody>;

    fn total_file_changes_limit(&self) -> Option<RateLimitBody>;
}

define_stats! {
    load_shed_counter: dynamic_singleton_counter("{}", (key: String)),
}

#[derive(Clone)]
pub struct RateLimitEnvironment {
    fb: FacebookInit,
    category: String,
    config: ConfigHandle<MononokeRateLimitConfig>,
}

impl RateLimitEnvironment {
    pub fn new(
        fb: FacebookInit,
        category: String,
        config: ConfigHandle<MononokeRateLimitConfig>,
    ) -> Self {
        Self {
            fb,
            category,
            config,
        }
    }

    pub fn get_rate_limiter(&self) -> BoxRateLimiter {
        let config = self.config.get();

        create_rate_limiter(self.fb, self.category.clone(), config)
    }
}

#[derive(Debug, Clone)]
pub struct RateLimitBody {
    pub raw_config: rate_limiting_config::RateLimitBody,
    pub window: Duration,
}

#[derive(Debug, Clone)]
pub struct MononokeRateLimitConfig {
    pub region_weight: f64,
    pub rate_limits: Vec<RateLimit>,
    pub load_shed_limits: Vec<LoadShedLimit>,
    #[allow(dead_code)]
    commits_per_author: RateLimitBody,
    #[allow(dead_code)]
    total_file_changes: Option<RateLimitBody>,
}

#[derive(Debug, Clone)]
pub struct RateLimit {
    pub body: RateLimitBody,
    #[allow(dead_code)]
    target: Option<Target>,
    #[allow(dead_code)]
    metric: Metric,
}

#[cfg(fbcode_build)]
impl RateLimit {
    fn applies_to_client(&self, identities: &MononokeIdentitySet) -> bool {
        match &self.target {
            // TODO (harveyhunt): Pass identities rather than Some(identities) once LFS server has
            // been updated to require certs.
            Some(t) => t.matches_client(Some(identities)),
            None => true,
        }
    }
}

impl LoadShedLimit {
    // TODO(harveyhunt): Make identities none optional once LFS server enforces that.
    pub fn should_load_shed(
        &self,
        fb: FacebookInit,
        identities: Option<&MononokeIdentitySet>,
    ) -> Result<(), RateLimitReason> {
        let applies_to_client = match &self.target {
            Some(t) => t.matches_client(identities),
            None => true,
        };

        if !applies_to_client {
            return Ok(());
        }

        let metric = self.raw_config.metric.to_string();

        match STATS::load_shed_counter.get_value(fb, (metric.clone(),)) {
            Some(value) if value > self.raw_config.limit => Err(RateLimitReason::LoadShedMetric(
                metric,
                value,
                self.raw_config.limit,
            )),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadShedLimit {
    pub raw_config: rate_limiting_config::LoadShedLimit,
    target: Option<Target>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Metric {
    EgressBytes,
    TotalManifests,
    GetpackFiles,
    Commits,
}

#[must_use]
#[derive(Debug, Error)]
pub enum RateLimitReason {
    #[error("Rate limited by {0:?} over {1:?}")]
    RateLimitedMetric(Metric, Duration),
    #[error("Load shed due to {0} (value: {1}, limit: {2})")]
    LoadShedMetric(String, i64, i64),
}

#[derive(Debug, Clone)]
pub enum Target {
    NotTarget(Box<Target>),
    AndTarget(Vec<Target>),
    OrTarget(Vec<Target>),
    Identity(MononokeIdentity),
    StaticSlice(StaticSlice),
}

#[derive(Debug, Copy, Clone)]
struct SlicePct(u8);

impl TryFrom<i32> for SlicePct {
    type Error = Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if !(0..=100).contains(&value) {
            return Err(anyhow!("Invalid percentage"));
        }

        Ok(Self(value.try_into()?))
    }
}

#[derive(Debug, Clone)]
pub struct StaticSlice {
    slice_pct: SlicePct,
    // This is hashed with a client's hostname to allow us to change
    // which percentage of hosts are in a static slice.
    nonce: String,
}

impl Target {
    pub fn matches_client(&self, identities: Option<&MononokeIdentitySet>) -> bool {
        match self {
            Self::NotTarget(t) => !t.matches_client(identities),
            Self::AndTarget(ts) => ts.iter().all(|t| t.matches_client(identities)),
            Self::OrTarget(ts) => ts.iter().any(|t| t.matches_client(identities)),
            Self::Identity(i) => match identities {
                Some(client_idents) => client_idents.contains(i),
                None => false,
            },
            Self::StaticSlice(s) => in_throttled_slice(identities, s.slice_pct, &s.nonce),
        }
    }
}

fn in_throttled_slice(
    identities: Option<&MononokeIdentitySet>,
    slice_pct: SlicePct,
    nonce: &str,
) -> bool {
    let hostname = if let Some(hostname) = identities.map(|i| i.hostname()) {
        hostname
    } else {
        return false;
    };

    let mut hasher = DefaultHasher::new();
    hostname.hash(&mut hasher);
    nonce.hash(&mut hasher);

    hasher.finish() % 100 < slice_pct.0.into()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_target_matches() {
        let test_ident = MononokeIdentity::new("USER", "foo");
        let test2_ident = MononokeIdentity::new("USER", "bar");
        let test3_ident = MononokeIdentity::new("USER", "baz");

        let ident_target = Target::Identity(test_ident.clone());
        let ident2_target = Target::Identity(test2_ident);
        let ident3_target = Target::Identity(test3_ident.clone());
        let empty_idents = Some(MononokeIdentitySet::new());

        assert!(!ident_target.matches_client(empty_idents.as_ref()));

        let mut idents = MononokeIdentitySet::new();
        idents.insert(test_ident);
        idents.insert(test3_ident);
        let idents = Some(idents);

        assert!(ident_target.matches_client(idents.as_ref()));

        let and_target = Target::AndTarget(vec![ident_target.clone(), ident3_target]);

        assert!(and_target.matches_client(idents.as_ref()));

        let or_target = Target::OrTarget(vec![ident_target, ident2_target.clone()]);

        assert!(or_target.matches_client(idents.as_ref()));

        let not_target = Target::NotTarget(Box::new(ident2_target));
        assert!(not_target.matches_client(idents.as_ref()));
    }

    #[test]
    fn test_target_in_static_slice() {
        let mut identities = MononokeIdentitySet::new();
        identities.insert(MononokeIdentity::new("MACHINE", "abc123.abc1.facebook.com"));

        assert!(!in_throttled_slice(None, 100.try_into().unwrap(), "abc"));

        assert!(!in_throttled_slice(
            Some(&identities),
            0.try_into().unwrap(),
            "abc"
        ));

        assert!(in_throttled_slice(
            Some(&identities),
            100.try_into().unwrap(),
            "abc"
        ));

        assert!(in_throttled_slice(
            Some(&identities),
            50.try_into().unwrap(),
            "123"
        ));

        // Check that changing the nonce results in a different slice.
        assert!(!in_throttled_slice(
            Some(&identities),
            50.try_into().unwrap(),
            "abc"
        ));
    }
}
