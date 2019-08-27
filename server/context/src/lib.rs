// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![feature(never_type)]

use chashmap::CHashMap;
use failure_ext::Error;
use fbwhoami::FbWhoAmI;
use limits::types::MononokeThrottleLimit;
use ratelim::loadlimiter::{self, LoadCost, LoadLimitCounter};
use serde::{Serialize, Serializer};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use futures::{self, Future, IntoFuture};
use futures_ext::FutureExt;
use scuba_ext::ScubaSampleBuilder;
use slog::{o, Logger, OwnedKV, SendSyncRefUnwindSafeKV};
use sshrelay::SshEnvVars;
use tracing::TraceContext;
use uuid::Uuid;

#[derive(Debug)]
pub enum Metric {
    EgressBytes,
    IngressBlobstoreBytes,
    EgressTotalManifests,
}

/// Creates a regional key to be used for load limiting, based on the given prefix.
///   myprefix -> myprefix:lla
fn make_limit_key(prefix: &str) -> String {
    let region = FbWhoAmI::new()
        .unwrap()
        .get_region_data_center_prefix()
        .unwrap();
    let mut key = prefix.to_owned();
    key.push_str(":");
    key.push_str(region);
    key
}

struct LoadLimiter {
    egress_bytes: LoadLimitCounter,
    ingress_blobstore_bytes: LoadLimitCounter,
    egress_total_manifests: LoadLimitCounter,
    category: String,
    limits: MononokeThrottleLimit,
}

impl LoadLimiter {
    fn new(limits: MononokeThrottleLimit, category: String) -> Self {
        Self {
            egress_bytes: LoadLimitCounter {
                category: category.clone(),
                key: make_limit_key("egress-bytes"),
            },
            ingress_blobstore_bytes: LoadLimitCounter {
                category: category.clone(),
                key: make_limit_key("ingress-blobstore-bytes"),
            },
            egress_total_manifests: LoadLimitCounter {
                category: category.clone(),
                key: make_limit_key("egress-total-manifests"),
            },
            category,
            limits,
        }
    }

    /// Translate a Metric to a resource configuration that can be used by SCS (Structured
    /// Counting Service)
    fn counter(&self, metric: Metric) -> &LoadLimitCounter {
        match metric {
            Metric::EgressBytes => &self.egress_bytes,
            Metric::IngressBlobstoreBytes => &self.ingress_blobstore_bytes,
            Metric::EgressTotalManifests => &self.egress_total_manifests,
        }
    }

    pub fn should_throttle(
        &self,
        metric: Metric,
        window: Duration,
    ) -> impl Future<Item = bool, Error = Error> {
        let limit = match metric {
            Metric::EgressBytes => self.limits.egress_bytes,
            Metric::IngressBlobstoreBytes => self.limits.ingress_blobstore_bytes,
            Metric::EgressTotalManifests => self.limits.total_manifests,
        };

        loadlimiter::should_throttle(&self.counter(metric), limit, window)
    }

    pub fn bump_load(&self, metric: Metric, load: LoadCost) {
        loadlimiter::bump_load(&self.counter(metric), load)
    }
}

impl fmt::Debug for LoadLimiter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LoadLimiter")
            .field("category", &self.category)
            .field("limits", &self.limits)
            .finish()
    }
}

pub fn is_quicksand(ssh_env_vars: &SshEnvVars) -> bool {
    if let Some(ref ssh_cert_principals) = ssh_env_vars.ssh_cert_principals {
        ssh_cert_principals.contains("quicksand")
    } else {
        false
    }
}

#[derive(Debug, Clone)]
pub struct CoreContext {
    inner: Arc<Inner>,
}

#[derive(Debug, Clone)]
pub struct PerfCounters {
    // A wrapper around a concurrent HashMap that allows for
    // tracking of arbitrary counters.
    counters: CHashMap<&'static str, i64>,
}

impl Serialize for PerfCounters {
    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_map(self.counters.clone().into_iter())
    }
}

impl PerfCounters {
    pub fn new() -> Self {
        Self {
            counters: CHashMap::new(),
        }
    }

    pub fn change_counter<F>(&self, name: &'static str, default: i64, update: F)
    where
        F: FnOnce(&mut i64),
    {
        self.counters.upsert(name, || default, update);
    }

    pub fn set_counter(&self, name: &'static str, val: i64) {
        self.counters.insert(name, val);
    }

    pub fn increment_counter(&self, name: &'static str) {
        self.add_to_counter(name, 1);
    }

    pub fn decrement_counter(&self, name: &'static str) {
        self.add_to_counter(name, -1);
    }

    pub fn add_to_counter(&self, name: &'static str, val: i64) {
        self.change_counter(name, val, |old| *old += val);
    }

    pub fn is_empty(&self) -> bool {
        self.counters.is_empty()
    }

    pub fn set_max_counter(&self, name: &'static str, val: i64) {
        self.change_counter(name, val, |old| {
            if val > *old {
                *old = val
            }
        });
    }
}

#[derive(Clone)]
struct Inner {
    session: Uuid,
    logger: Logger,
    scuba: ScubaSampleBuilder,
    // Logging some prod wireproto requests to scribe so that they can be later replayed on
    // shadow tier.
    wireproto_scribe_category: Option<String>,
    trace: TraceContext,
    perf_counters: PerfCounters,
    user_unix_name: Option<String>,
    ssh_env_vars: SshEnvVars,
    load_limiter: Option<Arc<LoadLimiter>>,
}

impl ::std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        write!(
            f,
            "CoreContext::Inner
            session: {:?}
            wireproto scribe category: {:?}
            perf counters: {:?}
            user unix name: {:?}
            ssh_env_vars: {:?}
            load_limiter: {:?}
            ",
            self.session,
            self.wireproto_scribe_category,
            self.perf_counters,
            self.user_unix_name,
            self.ssh_env_vars,
            self.load_limiter,
        )
    }
}

impl CoreContext {
    pub fn new(
        session: Uuid,
        logger: Logger,
        scuba: ScubaSampleBuilder,
        wireproto_scribe_category: Option<String>,
        trace: TraceContext,
        user_unix_name: Option<String>,
        ssh_env_vars: SshEnvVars,
        load_limiter: Option<(MononokeThrottleLimit, String)>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                session,
                logger,
                scuba,
                wireproto_scribe_category,
                trace,
                perf_counters: PerfCounters::new(),
                user_unix_name,
                ssh_env_vars,
                load_limiter: load_limiter
                    .map(|(limits, category)| Arc::new(LoadLimiter::new(limits, category))),
            }),
        }
    }

    pub fn new_with_logger(logger: Logger) -> Self {
        Self::new(
            Uuid::new_v4(),
            logger,
            ScubaSampleBuilder::with_discard(),
            None,
            TraceContext::default(),
            None,
            SshEnvVars::default(),
            None,
        )
    }

    pub fn with_logger_kv<T>(&self, values: OwnedKV<T>) -> Self
    where
        T: SendSyncRefUnwindSafeKV + 'static,
    {
        Self {
            inner: Arc::new(Inner {
                session: self.inner.session.clone(),
                logger: self.inner.logger.new(values),
                scuba: self.inner.scuba.clone(),
                wireproto_scribe_category: self.inner.wireproto_scribe_category.clone(),
                trace: self.inner.trace.clone(),
                perf_counters: self.inner.perf_counters.clone(),
                user_unix_name: self.inner.user_unix_name.clone(),
                ssh_env_vars: self.inner.ssh_env_vars.clone(),
                load_limiter: self.inner.load_limiter.clone(),
            }),
        }
    }

    pub fn with_scuba_initialization<F>(&self, init: F) -> Self
    where
        F: FnOnce(ScubaSampleBuilder) -> ScubaSampleBuilder,
    {
        Self {
            inner: Arc::new(Inner {
                session: self.inner.session.clone(),
                logger: self.inner.logger.clone(),
                scuba: init(self.inner.scuba.clone()),
                wireproto_scribe_category: self.inner.wireproto_scribe_category.clone(),
                trace: self.inner.trace.clone(),
                perf_counters: self.inner.perf_counters.clone(),
                user_unix_name: self.inner.user_unix_name.clone(),
                ssh_env_vars: self.inner.ssh_env_vars.clone(),
                load_limiter: self.inner.load_limiter.clone(),
            }),
        }
    }

    pub fn test_mock() -> Self {
        Self::new(
            Uuid::new_v4(),
            Logger::root(::slog::Discard, o!()),
            ScubaSampleBuilder::with_discard(),
            None,
            TraceContext::default(),
            None,
            SshEnvVars::default(),
            None,
        )
    }

    pub fn session(&self) -> &Uuid {
        &self.inner.session
    }
    pub fn logger(&self) -> &Logger {
        &self.inner.logger
    }
    pub fn scuba(&self) -> &ScubaSampleBuilder {
        &self.inner.scuba
    }
    pub fn wireproto_scribe_category(&self) -> &Option<String> {
        &self.inner.wireproto_scribe_category
    }
    pub fn trace(&self) -> &TraceContext {
        &self.inner.trace
    }
    pub fn perf_counters(&self) -> &PerfCounters {
        &self.inner.perf_counters
    }
    pub fn user_unix_name(&self) -> &Option<String> {
        &self.inner.user_unix_name
    }
    pub fn ssh_env_vars(&self) -> &SshEnvVars {
        &self.inner.ssh_env_vars
    }
    pub fn is_quicksand(&self) -> bool {
        is_quicksand(self.ssh_env_vars())
    }
    pub fn bump_load(&self, metric: Metric, load: LoadCost) {
        if let Some(limiter) = &self.inner.load_limiter {
            limiter.bump_load(metric, load)
        }
    }
    pub fn should_throttle(
        &self,
        metric: Metric,
        duration: Duration,
    ) -> impl Future<Item = bool, Error = !> {
        match &self.inner.load_limiter {
            Some(limiter) => limiter
                .should_throttle(metric, duration)
                .then(|res| {
                    let r: Result<_, !> = match res {
                        Ok(res) => Ok(res),
                        Err(_) => Ok(false),
                    };
                    r
                })
                .left_future(),
            None => Ok(false).into_future().right_future(),
        }
    }
}
