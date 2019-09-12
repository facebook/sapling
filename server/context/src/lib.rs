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
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    EgressGetfilesFiles,
    EgressGetpackFiles,
    EgressCommits,
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
    egress_getfiles_files: LoadLimitCounter,
    egress_getpack_files: LoadLimitCounter,
    egress_commits: LoadLimitCounter,
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
            egress_getfiles_files: LoadLimitCounter {
                category: category.clone(),
                key: make_limit_key("egress-getfiles-files"),
            },
            egress_getpack_files: LoadLimitCounter {
                category: category.clone(),
                key: make_limit_key("egress-getpack-files"),
            },
            egress_commits: LoadLimitCounter {
                category: category.clone(),
                key: make_limit_key("egress-commits"),
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
            Metric::EgressGetfilesFiles => &self.egress_getfiles_files,
            Metric::EgressGetpackFiles => &self.egress_getpack_files,
            Metric::EgressCommits => &self.egress_commits,
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
            Metric::EgressGetfilesFiles => self.limits.getfiles_files,
            Metric::EgressGetpackFiles => self.limits.getpack_files,
            Metric::EgressCommits => self.limits.commits,
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

macro_rules! enum_str {
    (enum $name:ident {
        $($variant:ident),*,
    }) => {
        #[derive(Debug, Clone, PartialEq, Hash)]
        pub enum $name {
            $($variant),*
        }

        impl $name {
            pub fn name(&self) -> &'static str {
                match self {
                    $($name::$variant => stringify!($variant)),*
                }
            }
        }
    };
}

enum_str! {
    enum PerfCounterType {
        BlobstoreGets,
        BlobstoreGetsMaxLatency,
        BlobstorePresenceChecks,
        BlobstorePresenceChecksMaxLatency,
        BlobstorePuts,
        BlobstorePutsMaxLatency,
        GetbundleNumCommits,
        GetfilesMaxFileSize,
        GetfilesMaxLatency,
        GetfilesNumFiles,
        GetfilesResponseSize,
        GettreepackResponseSize,
        GettreepackNumTreepacks,
        GetpackMaxFileSize,
        GetpackNumFiles,
        GetpackResponseSize,
        SkiplistAncestorGen,
        SkiplistDescendantGen,
        SkiplistNoskipIterations,
        SkiplistSkipIterations,
        SkiplistSkippedGenerations,
        SumManifoldPollTime,
        SqlGetsReplica,
        SqlGetsMaster,
        SqlInserts,
    }
}

impl PerfCounterType {
    pub(crate) fn log_in_separate_column(&self) -> bool {
        use PerfCounterType::*;

        match self {
            BlobstoreGets
            | BlobstoreGetsMaxLatency
            | BlobstorePresenceChecks
            | BlobstorePresenceChecksMaxLatency
            | BlobstorePuts
            | BlobstorePutsMaxLatency
            | SqlGetsReplica
            | SqlGetsMaster
            | SqlInserts => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PerfCounters {
    // A wrapper around a concurrent HashMap that allows for
    // tracking of arbitrary counters.
    counters: CHashMap<PerfCounterType, i64>,
}

impl PerfCounters {
    pub fn new() -> Self {
        Self {
            counters: CHashMap::new(),
        }
    }

    pub fn change_counter<F>(&self, counter: PerfCounterType, default: i64, update: F)
    where
        F: FnOnce(&mut i64),
    {
        self.counters.upsert(counter, || default, update);
    }

    pub fn set_counter(&self, counter: PerfCounterType, val: i64) {
        self.counters.insert(counter, val);
    }

    pub fn increment_counter(&self, counter: PerfCounterType) {
        self.add_to_counter(counter, 1);
    }

    pub fn decrement_counter(&self, counter: PerfCounterType) {
        self.add_to_counter(counter, -1);
    }

    pub fn add_to_counter(&self, counter: PerfCounterType, val: i64) {
        self.change_counter(counter, val, |old| *old += val);
    }

    pub fn is_empty(&self) -> bool {
        self.counters.is_empty()
    }

    pub fn set_max_counter(&self, counter: PerfCounterType, val: i64) {
        self.change_counter(counter, val, |old| {
            if val > *old {
                *old = val
            }
        });
    }

    pub fn insert_perf_counters(&self, builder: &mut ScubaSampleBuilder) {
        let mut extra = HashMap::new();
        for (key, value) in self.counters.clone().into_iter() {
            if key.log_in_separate_column() {
                builder.add(key.name(), format!("{}", value));
            } else {
                extra.insert(key.name(), value);
            }
        }

        if let Ok(extra) = serde_json::to_string(&extra) {
            // Scuba does not support columns that are too long, we have to trim it
            let limit = ::std::cmp::min(extra.len(), 1000);
            builder.add("extra_context", &extra[..limit]);
        }
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
        let session_uuid = Uuid::new_v4();
        let trace = TraceContext::new(session_uuid, Instant::now());

        Self::new(
            Uuid::new_v4(),
            logger,
            ScubaSampleBuilder::with_discard(),
            None,
            trace,
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
