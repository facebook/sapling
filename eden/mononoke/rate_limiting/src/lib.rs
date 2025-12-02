/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::string::ToString;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use anyhow::Error;
use anyhow::anyhow;
use async_trait::async_trait;
use cached_config::ConfigHandle;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use ods_counters::CounterManager;
use ods_counters::OdsCounterManager;
use ods_counters::periodic_fetch_counter;
use permission_checker::MononokeIdentitySet;
use permission_checker::MononokeIdentitySetExt;
use rate_limiting_config::ExternalOdsCounter;
use scuba_ext::MononokeScubaSampleBuilder;
use stats::prelude::*;
use thiserror::Error;

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;

#[cfg(fbcode_build)]
pub use facebook::create_rate_limiter;
#[cfg(not(fbcode_build))]
pub use oss::create_rate_limiter;
pub use rate_limiting_config::LoadSheddingMetric;
pub use rate_limiting_config::RateLimitStatus;

pub mod config;

pub type LoadCost = f64;
pub type BoxRateLimiter = Box<dyn RateLimiter + Send + Sync + 'static>;

pub enum RateLimitResult {
    Pass,
    Fail(RateLimitReason),
}

#[async_trait]
pub trait RateLimiter {
    async fn check_rate_limit(
        &self,
        metric: Metric,
        identities: &MononokeIdentitySet,
        main_id: Option<&str>,
        scuba: &mut MononokeScubaSampleBuilder,
        atlas: Option<bool>,
    ) -> Result<RateLimitResult, Error>;

    fn check_load_shed(
        &self,
        identities: &MononokeIdentitySet,
        main_id: Option<&str>,
        scuba: &mut MononokeScubaSampleBuilder,
        atlas: Option<bool>,
    ) -> LoadShedResult;

    fn bump_load(&self, metric: Metric, scope: Scope, load: LoadCost);

    fn category(&self) -> &str;

    fn find_rate_limit(
        &self,
        metric: Metric,
        identities: Option<MononokeIdentitySet>,
        main_id: Option<&str>,
        atlas: Option<bool>,
    ) -> Option<RateLimit>;
}

define_stats! {
    load_shed_counter: dynamic_singleton_counter("{}", (key: String)),
}

#[derive(Clone)]
pub struct RateLimitEnvironment {
    fb: FacebookInit,
    category: String,
    config: ConfigHandle<MononokeRateLimitConfig>,
    counter_manager: Arc<RwLock<OdsCounterManager>>,
}

impl RateLimitEnvironment {
    pub fn new(
        fb: FacebookInit,
        category: String,
        config: ConfigHandle<MononokeRateLimitConfig>,
        counter_manager: Arc<RwLock<OdsCounterManager>>,
    ) -> Self {
        for limit in &config.get().load_shed_limits {
            match &limit.raw_config.load_shedding_metric {
                LoadSheddingMetric::external_ods_counter(counter) => {
                    counter_manager.write().expect("Poisoned lock").add_counter(
                        counter.entity.clone(),
                        counter.key.clone(),
                        counter.reduce.clone(),
                    )
                }
                _ => {}
            };
        }

        mononoke::spawn_task(periodic_fetch_counter(
            counter_manager.clone(),
            Duration::from_secs(60),
        ));

        Self {
            fb,
            category,
            config,
            counter_manager,
        }
    }

    pub fn new_with_runtime(
        fb: FacebookInit,
        category: String,
        config: ConfigHandle<MononokeRateLimitConfig>,
        counter_manager: Arc<RwLock<OdsCounterManager>>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        for limit in &config.get().load_shed_limits {
            match &limit.raw_config.load_shedding_metric {
                LoadSheddingMetric::external_ods_counter(counter) => {
                    counter_manager.write().expect("Poisoned lock").add_counter(
                        counter.entity.clone(),
                        counter.key.clone(),
                        counter.reduce.clone(),
                    )
                }
                _ => {}
            };
        }

        runtime.spawn(periodic_fetch_counter(
            counter_manager.clone(),
            Duration::from_secs(60),
        ));

        Self {
            fb,
            category,
            config,
            counter_manager,
        }
    }

    pub fn get_rate_limiter(&self) -> BoxRateLimiter {
        let config = self.config.get();

        create_rate_limiter(
            self.fb,
            self.category.clone(),
            config,
            self.counter_manager.clone(),
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RateLimitBody {
    pub raw_config: rate_limiting_config::RateLimitBody,
}

#[derive(Debug, Clone)]
pub struct MononokeRateLimitConfig {
    pub rate_limits: Vec<RateLimit>,
    pub load_shed_limits: Vec<LoadShedLimit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RateLimit {
    pub body: RateLimitBody,
    #[allow(dead_code)]
    pub target: Option<Target>,
    #[allow(dead_code)]
    pub fci_metric: FciMetric,
}

#[cfg(fbcode_build)]
impl RateLimit {
    fn applies_to_client(
        &self,
        identities: &MononokeIdentitySet,
        main_id: Option<&str>,
        atlas: Option<bool>,
    ) -> bool {
        match &self.target {
            // TODO (harveyhunt): Pass identities rather than Some(identities) once LFS server has
            // been updated to require certs.
            Some(t) => t.matches_client(Some(identities), main_id, atlas),
            None => true,
        }
    }
}

pub enum LoadShedResult {
    Pass,
    Fail(RateLimitReason),
}

pub fn log_or_enforce_status(
    raw_config: rate_limiting_config::LoadShedLimit,
    metric: String,
    value: i64,
    scuba: &mut MononokeScubaSampleBuilder,
) -> LoadShedResult {
    match raw_config.status {
        RateLimitStatus::Disabled => LoadShedResult::Pass,
        RateLimitStatus::Tracked => {
            scuba.log_with_msg(
                "Would have rate limited",
                format!(
                    "{:?}",
                    (RateLimitReason::LoadShedMetric(metric, value, raw_config.limit,))
                ),
            );
            LoadShedResult::Pass
        }
        RateLimitStatus::Enforced => LoadShedResult::Fail(RateLimitReason::LoadShedMetric(
            metric,
            value,
            raw_config.limit,
        )),
        _ => panic!(
            "Thrift enums aren't real enums once in Rust. We have to account for other values here."
        ),
    }
}

impl LoadShedLimit {
    // TODO(harveyhunt): Make identities none optional once LFS server enforces that.
    pub fn should_load_shed(
        &self,
        fb: FacebookInit,
        identities: Option<&MononokeIdentitySet>,
        main_id: Option<&str>,
        scuba: &mut MononokeScubaSampleBuilder,
        ods_counters: Arc<RwLock<OdsCounterManager>>,
        atlas: Option<bool>,
    ) -> LoadShedResult {
        let applies_to_client = match &self.target {
            Some(t) => t.matches_client(identities, main_id, atlas),
            None => true,
        };

        if !applies_to_client {
            return LoadShedResult::Pass;
        }

        // Fetch the counter
        let (metric_string, value) = match self.raw_config.load_shedding_metric.clone() {
            LoadSheddingMetric::local_fb303_counter(metric) => {
                let metric = metric.to_string();
                (
                    metric.clone(),
                    STATS::load_shed_counter.get_value(fb, (metric,)),
                )
            }
            LoadSheddingMetric::external_ods_counter(ExternalOdsCounter {
                entity,
                key,
                reduce,
            }) => {
                let value = ods_counters
                    .read()
                    .expect("Poisoned lock")
                    .get_counter_value(&entity, &key, reduce.as_deref())
                    .map(|v| v as i64);
                (
                    format!("Ods key:{} entity:{} reduce:{:?}", entity, key, reduce),
                    value,
                )
            }
            _ => ("".to_string(), None),
        };

        match value {
            Some(value) if value > self.raw_config.limit => {
                log_or_enforce_status(self.raw_config.clone(), metric_string, value, scuba)
            }
            _ => LoadShedResult::Pass,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadShedLimit {
    pub raw_config: rate_limiting_config::LoadShedLimit,
    target: Option<Target>,
}

#[derive(Debug, Copy, Clone, PartialEq, strum::Display)]
pub enum Metric {
    EgressBytes,
    TotalManifests,
    GetpackFiles,
    Commits,
    CommitsPerAuthor,
    CommitsPerUser,
    EdenApiQps,
    LocationToHashCount,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Scope {
    Global,
    Regional,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FciMetric {
    pub metric: Metric,
    pub window: Duration,
    pub scope: Scope,
}

#[must_use]
#[derive(Debug, Error)]
pub enum RateLimitReason {
    #[error("Rate limited by {0:?} over {1:?}")]
    RateLimitedMetric(Metric, Duration),
    #[error("Load shed due to {0} (value: {1}, limit: {2})")]
    LoadShedMetric(String, i64, i64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    StaticSlice(StaticSlice),
    MainClientId(String),
    Identities(MononokeIdentitySet),
    Atlas(AtlasTarget),
}

#[derive(Debug, Copy, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct StaticSlice {
    slice_pct: SlicePct,
    // This is hashed with a client's hostname to allow us to change
    // which percentage of hosts are in a static slice.
    nonce: String,
    target: StaticSliceTarget,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AtlasTarget {
    // Empty struct - applies to all Atlas environments
}

#[derive(Debug, Clone, PartialEq)]
pub enum StaticSliceTarget {
    Identities(MononokeIdentitySet),
    MainClientId(String),
}

impl Target {
    pub fn matches_client(
        &self,
        identities: Option<&MononokeIdentitySet>,
        main_client_id: Option<&str>,
        atlas: Option<bool>,
    ) -> bool {
        match self {
            Self::Identities(target_identities) => {
                if target_identities.is_empty() {
                    true
                } else {
                    match identities {
                        Some(identities) => {
                            // Check that identities is a subset of client_idents
                            target_identities.is_subset(identities)
                        }
                        None => false,
                    }
                }
            }
            Self::MainClientId(id) => match main_client_id {
                Some(client_id) => client_id == id,
                None => false,
            },
            Self::StaticSlice(s) => {
                // Check that identities is a subset of client_idents
                match matches_static_slice_target(&s.target, identities, main_client_id) {
                    true => in_throttled_slice(identities, s.slice_pct, &s.nonce),
                    false => false,
                }
            }
            Self::Atlas(_atlas_target) => {
                // Check if the client is an Atlas client
                atlas == Some(true)
            }
        }
    }
}

fn matches_static_slice_target(
    target: &StaticSliceTarget,
    identities: Option<&MononokeIdentitySet>,
    main_client_id: Option<&str>,
) -> bool {
    match target {
        StaticSliceTarget::Identities(target_identities) => {
            match identities {
                Some(identities) => {
                    // Check that identities is a subset of client_idents
                    target_identities.is_subset(identities)
                }
                None => false,
            }
        }
        StaticSliceTarget::MainClientId(id) => match main_client_id {
            Some(client_id) => client_id == id,
            None => false,
        },
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
    #[cfg(fbcode_build)]
    use std::sync::Arc;

    use mononoke_macros::mononoke;
    use permission_checker::MononokeIdentity;

    use super::*;

    #[mononoke::test]
    fn test_target_matches() {
        let test_ident = MononokeIdentity::new("USER", "foo");
        let test2_ident = MononokeIdentity::new("USER", "baz");
        let test_client_id = String::from("test_client_id");
        let empty_idents = Some(MononokeIdentitySet::new());

        let ident_target = Target::Identities([test_ident.clone()].into());

        assert!(!ident_target.matches_client(empty_idents.as_ref(), None, None));

        let mut idents = MononokeIdentitySet::new();
        idents.insert(test_ident.clone());
        idents.insert(test2_ident.clone());
        let idents = Some(idents);

        assert!(ident_target.matches_client(idents.as_ref(), None, None));

        let two_idents = Target::Identities([test_ident, test2_ident].into());

        assert!(two_idents.matches_client(idents.as_ref(), None, None));

        let client_id_target = Target::MainClientId(test_client_id.clone());
        assert!(client_id_target.matches_client(None, Some(&test_client_id), None));

        // Check that all match if the target is empty.
        let empty_ident_target = Target::Identities([].into());
        assert!(empty_ident_target.matches_client(None, None, None));
        assert!(empty_ident_target.matches_client(idents.as_ref(), None, None));
        assert!(empty_ident_target.matches_client(None, Some(&test_client_id), None));
        assert!(empty_ident_target.matches_client(idents.as_ref(), Some(&test_client_id), None));
    }

    #[mononoke::test]
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

    #[mononoke::test]
    fn test_atlas_target_matches() {
        // Test Atlas target - matches all Atlas
        let atlas_target = Target::Atlas(AtlasTarget {});

        // Should match any Atlas client
        assert!(atlas_target.matches_client(None, None, Some(true)));

        // Should not match non-Atlas clients
        assert!(!atlas_target.matches_client(None, None, Some(false)));
        assert!(!atlas_target.matches_client(None, None, None));
    }

    #[cfg(fbcode_build)]
    #[mononoke::test]
    fn test_static_slice_of_identity_set() {
        let test_ident = MononokeIdentity::new("USER", "foo");
        let test2_ident = MononokeIdentity::new("SERVICE_IDENTITY", "bar");
        let test3_ident = MononokeIdentity::new("MACHINE", "abc125.abc.facebook.com");
        let test4_ident = MononokeIdentity::new("MACHINE", "abc124.abc.facebook.com");

        let ident_target = Target::Identities([test2_ident.clone()].into());
        let twenty_pct_service_identity = Target::StaticSlice(StaticSlice {
            slice_pct: 20.try_into().unwrap(),
            nonce: "nonce".into(),
            target: StaticSliceTarget::Identities([test2_ident.clone()].into()),
        });
        let hundred_pct_service_identity = Target::StaticSlice(StaticSlice {
            slice_pct: 100.try_into().unwrap(),
            nonce: "nonce".into(),
            target: StaticSliceTarget::Identities([test2_ident.clone()].into()),
        });

        let mut idents = MononokeIdentitySet::new();
        idents.insert(test_ident.clone());
        idents.insert(test2_ident.clone());
        idents.insert(test3_ident);
        let idents1 = Some(idents);

        let mut idents = MononokeIdentitySet::new();
        idents.insert(test_ident);
        idents.insert(test2_ident);
        idents.insert(test4_ident);
        let idents2 = Some(idents);

        // All of SERVICE_IDENTITY: bar
        assert!(ident_target.matches_client(idents1.as_ref(), None, None));

        // 20% of SERVICE_IDENTITY: bar. ratelimited host
        assert!(twenty_pct_service_identity.matches_client(idents1.as_ref(), None, None));

        // 20% of SERVICE_IDENTITY: bar. not ratelimited host
        assert!(!twenty_pct_service_identity.matches_client(idents2.as_ref(), None, None));

        // 100% of SERVICE_IDENTITY: bar
        assert!(hundred_pct_service_identity.matches_client(idents1.as_ref(), None, None));

        // 100% of SERVICE_IDENTITY: bar
        assert!(hundred_pct_service_identity.matches_client(idents2.as_ref(), None, None));
    }

    #[cfg(fbcode_build)]
    #[mononoke::fbinit_test]
    fn test_find_rate_limit(fb: FacebookInit) {
        let main_client_id_rate_limit = RateLimit {
            body: RateLimitBody::default(),
            target: Some(Target::MainClientId("client_id".to_string())),
            fci_metric: FciMetric {
                metric: Metric::EgressBytes,
                window: Duration::from_secs(60),
                scope: Scope::Global,
            },
        };

        let identities_rate_limit = RateLimit {
            body: RateLimitBody::default(),
            target: Some(Target::Identities(
                [MononokeIdentity::new("TIER", "foo")].into(),
            )),
            fci_metric: FciMetric {
                metric: Metric::EgressBytes,
                window: Duration::from_secs(60),
                scope: Scope::Global,
            },
        };

        let empty_target_rate_limit = RateLimit {
            body: RateLimitBody::default(),
            target: Some(Target::Identities([].into())),
            fci_metric: FciMetric {
                metric: Metric::EgressBytes,
                window: Duration::from_secs(60),
                scope: Scope::Global,
            },
        };

        let rate_limiter = create_rate_limiter(
            fb,
            "test".to_string(),
            Arc::new(MononokeRateLimitConfig {
                rate_limits: vec![
                    main_client_id_rate_limit.clone(),
                    identities_rate_limit.clone(),
                    empty_target_rate_limit.clone(),
                ],
                load_shed_limits: vec![],
            }),
            OdsCounterManager::new(fb),
        );

        let mut idents = MononokeIdentitySet::new();
        idents.insert(MononokeIdentity::new("USER", "bar"));

        assert!(
            rate_limiter.find_rate_limit(
                Metric::EgressBytes,
                Some(idents.clone()),
                Some("non_matching_id"),
                None,
            ) == Some(empty_target_rate_limit)
        );
        assert!(
            rate_limiter.find_rate_limit(
                Metric::EgressBytes,
                Some(idents.clone()),
                Some("client_id"),
                None,
            ) == Some(main_client_id_rate_limit.clone())
        );

        idents.insert(MononokeIdentity::new("TIER", "foo"));
        assert!(
            rate_limiter.find_rate_limit(Metric::EgressBytes, Some(idents.clone()), None, None,)
                == Some(identities_rate_limit)
        );
        assert!(
            rate_limiter.find_rate_limit(
                Metric::EgressBytes,
                Some(idents),
                Some("client_id"),
                None,
            ) == Some(main_client_id_rate_limit)
        );
    }
}
