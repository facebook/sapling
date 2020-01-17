/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![feature(never_type, atomic_min_max)]

use anyhow::Error;
use fbinit::FacebookInit;
use fbwhoami::FbWhoAmI;
use identity::IdentitySet;
use limits::types::{MononokeThrottleLimit, RateLimits};
use ratelim::loadlimiter::{self, LoadCost, LoadLimitCounter};
use std::{fmt, sync::Arc, time::Duration};

use futures::{self, Future, IntoFuture};
use futures_ext::FutureExt;
use rand::{self, distributions::Alphanumeric, thread_rng, Rng};
use scuba_ext::ScubaSampleBuilder;
pub use session_id::SessionId;
use slog::{debug, o, warn, Logger};
use sshrelay::SshEnvVars;
use tracing::TraceContext;
use upload_trace::{manifold_thrift::thrift::RequestContext, UploadTrace};

pub use perf_counters::{PerfCounterType, PerfCounters};

mod perf_counters;

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

pub struct LoadLimiter {
    fb: FacebookInit,
    egress_bytes: LoadLimitCounter,
    ingress_blobstore_bytes: LoadLimitCounter,
    egress_total_manifests: LoadLimitCounter,
    egress_getfiles_files: LoadLimitCounter,
    egress_getpack_files: LoadLimitCounter,
    egress_commits: LoadLimitCounter,
    category: String,
    throttle_limits: MononokeThrottleLimit,
    rate_limits: RateLimits,
}

impl LoadLimiter {
    fn new(
        fb: FacebookInit,
        throttle_limits: MononokeThrottleLimit,
        rate_limits: RateLimits,
        category: String,
    ) -> Self {
        Self {
            fb,
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
            throttle_limits,
            rate_limits,
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
            Metric::EgressBytes => self.throttle_limits.egress_bytes,
            Metric::IngressBlobstoreBytes => self.throttle_limits.ingress_blobstore_bytes,
            Metric::EgressTotalManifests => self.throttle_limits.total_manifests,
            Metric::EgressGetfilesFiles => self.throttle_limits.getfiles_files,
            Metric::EgressGetpackFiles => self.throttle_limits.getpack_files,
            Metric::EgressCommits => self.throttle_limits.commits,
        };

        loadlimiter::should_throttle(self.fb, &self.counter(metric), limit, window)
    }

    pub fn bump_load(&self, metric: Metric, load: LoadCost) {
        loadlimiter::bump_load(self.fb, &self.counter(metric), load)
    }

    pub fn category(&self) -> &str {
        &self.category
    }

    pub fn rate_limits(&self) -> &RateLimits {
        &self.rate_limits
    }
}

impl fmt::Debug for LoadLimiter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LoadLimiter")
            .field("category", &self.category)
            .field("throttle_limits", &self.throttle_limits)
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

pub fn generate_session_id() -> SessionId {
    let s: String = thread_rng().sample_iter(&Alphanumeric).take(16).collect();
    SessionId::from_string(s)
}

#[derive(Debug)]
pub struct SessionContainerInner {
    session_id: SessionId,
    trace: TraceContext,
    user_unix_name: Option<String>,
    source_hostname: Option<String>,
    identities: Option<IdentitySet>,
    ssh_env_vars: SshEnvVars,
    load_limiter: Option<LoadLimiter>,
}

#[derive(Debug, Clone)]
pub struct SessionContainer {
    fb: FacebookInit,
    inner: Arc<SessionContainerInner>,
}

impl SessionContainer {
    pub fn new(
        fb: FacebookInit,
        session_id: SessionId,
        trace: TraceContext,
        user_unix_name: Option<String>,
        source_hostname: Option<String>,
        identities: Option<IdentitySet>,
        ssh_env_vars: SshEnvVars,
        load_limiter: Option<(MononokeThrottleLimit, RateLimits, String)>,
    ) -> Self {
        let load_limiter = load_limiter.map(|(throttle_limits, rate_limits, category)| {
            LoadLimiter::new(fb, throttle_limits, rate_limits, category)
        });

        let inner = SessionContainerInner {
            session_id,
            trace,
            user_unix_name,
            source_hostname,
            identities,
            ssh_env_vars,
            load_limiter,
        };

        Self {
            fb,
            inner: Arc::new(inner),
        }
    }

    pub fn new_with_defaults(fb: FacebookInit) -> Self {
        Self::new(
            fb,
            generate_session_id(),
            TraceContext::default(),
            None,
            None,
            None,
            SshEnvVars::default(),
            None,
        )
    }

    pub fn new_context(&self, logger: Logger, scuba: ScubaSampleBuilder) -> CoreContext {
        let logging = LoggingContainer::new(logger, scuba);

        CoreContext {
            fb: self.fb,
            logging,
            session: self.clone(),
        }
    }

    pub fn fb(&self) -> FacebookInit {
        self.fb
    }

    pub fn session_id(&self) -> &SessionId {
        &self.inner.session_id
    }

    pub fn trace(&self) -> &TraceContext {
        &self.inner.trace
    }

    pub fn user_unix_name(&self) -> &Option<String> {
        &self.inner.user_unix_name
    }

    pub fn source_hostname(&self) -> &Option<String> {
        &self.inner.source_hostname
    }

    pub fn identities(&self) -> &Option<IdentitySet> {
        &self.inner.identities
    }

    pub fn ssh_env_vars(&self) -> &SshEnvVars {
        &self.inner.ssh_env_vars
    }

    pub fn is_quicksand(&self) -> bool {
        is_quicksand(&self.inner.ssh_env_vars)
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

    pub fn load_limiter(&self) -> Option<&LoadLimiter> {
        match self.inner.load_limiter {
            Some(ref load_limiter) => Some(&load_limiter),
            None => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoggingContainer {
    logger: Logger,
    scuba: Arc<ScubaSampleBuilder>,
    perf_counters: Arc<PerfCounters>,
}

impl LoggingContainer {
    pub fn new(logger: Logger, scuba: ScubaSampleBuilder) -> Self {
        Self {
            logger,
            scuba: Arc::new(scuba),
            perf_counters: Arc::new(PerfCounters::default()),
        }
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn scuba(&self) -> &ScubaSampleBuilder {
        &self.scuba
    }

    pub fn perf_counters(&self) -> &PerfCounters {
        &self.perf_counters
    }
}

#[derive(Debug, Clone)]
pub struct CoreContext {
    pub fb: FacebookInit,
    session: SessionContainer,
    logging: LoggingContainer,
}

impl CoreContext {
    pub fn new_with_logger(fb: FacebookInit, logger: Logger) -> Self {
        let session = SessionContainer::new_with_defaults(fb);
        session.new_context(logger, ScubaSampleBuilder::with_discard())
    }

    pub fn test_mock(fb: FacebookInit) -> Self {
        let session = SessionContainer::new(
            fb,
            generate_session_id(),
            TraceContext::default(),
            None,
            None,
            None,
            SshEnvVars::default(),
            None,
        );

        session.new_context(
            Logger::root(::slog::Discard, o!()),
            ScubaSampleBuilder::with_discard(),
        )
    }

    pub fn clone_and_reset(&self) -> Self {
        self.session
            .new_context(self.logger().clone(), self.scuba().clone())
    }

    pub fn with_mutated_scuba(
        &self,
        sample: impl FnOnce(ScubaSampleBuilder) -> ScubaSampleBuilder,
    ) -> Self {
        self.session
            .new_context(self.logger().clone(), sample(self.scuba().clone()))
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session.session_id()
    }

    pub fn logger(&self) -> &Logger {
        &self.logging.logger()
    }

    pub fn scuba(&self) -> &ScubaSampleBuilder {
        &self.logging.scuba()
    }

    pub fn perf_counters(&self) -> &PerfCounters {
        &self.logging.perf_counters
    }

    pub fn trace(&self) -> &TraceContext {
        &self.session.trace()
    }

    pub fn user_unix_name(&self) -> &Option<String> {
        &self.session.user_unix_name()
    }

    pub fn identities(&self) -> &Option<IdentitySet> {
        &self.session.identities()
    }

    pub fn source_hostname(&self) -> &Option<String> {
        &self.session.source_hostname()
    }

    pub fn ssh_env_vars(&self) -> &SshEnvVars {
        &self.session.ssh_env_vars()
    }

    pub fn trace_upload(&self) -> impl Future<Item = (), Error = Error> {
        let logger = self.logger().clone();
        let id = self.trace().id().clone();
        self.trace()
            .upload_to_manifold(
                self.fb,
                RequestContext {
                    bucketName: "mononoke_prod".into(),
                    apiKey: "".into(),
                    ..Default::default()
                },
            )
            .then(move |result| match result {
                Err(err) => {
                    warn!(&logger, "failed to upload trace: {:#?}", err);
                    Err(err)
                }
                Ok(()) => {
                    debug!(
                        &logger,
                        "trace uploaded: https://our.intern.facebook.com/intern/mononoke/trace/{}",
                        id
                    );
                    Ok(())
                }
            })
    }

    pub fn session(&self) -> &SessionContainer {
        &self.session
    }
}
