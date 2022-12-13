/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use async_limiter::AsyncLimiter;
use fbinit::FacebookInit;
use metadata::Metadata;
use permission_checker::MononokeIdentitySetExt;
use rate_limiting::BoxRateLimiter;
use rate_limiting::LoadCost;
use rate_limiting::Metric;
use rate_limiting::RateLimitReason;
use rate_limiting::RateLimiter;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;

pub use self::builder::SessionContainerBuilder;
use crate::core::CoreContext;
use crate::logging::LoggingContainer;

mod builder;

#[derive(Clone)]
pub struct SessionContainer {
    fb: FacebookInit,
    inner: Arc<SessionContainerInner>,
    session_class: SessionClass,
}

/// Represents the reason this session is running
#[derive(Clone, Copy)]
pub enum SessionClass {
    /// There is someone waiting for this session to complete.
    UserWaiting,
    /// The session is doing background work (e.g. backfilling).
    /// Wherever reasonable, prefer to slow down and wait for work to complete
    /// fully rather than pushing work out to other tasks.
    Background,
    /// Same as Background, but if some work is taking too long to complete
    /// then fallback to normal (i.e. UserWaiting) behavior
    BackgroundUnlessTooSlow,
    /// This session is used by the warm bookmarks cache.
    WarmBookmarksCache,
    /// This session requires to check all multiplexed blobstores for is_present check.
    ComprehensiveLookup,
}

struct SessionContainerInner {
    metadata: Arc<Metadata>,
    rate_limiter: Option<BoxRateLimiter>,
    blobstore_write_limiter: Option<AsyncLimiter>,
    blobstore_read_limiter: Option<AsyncLimiter>,
    // Whether this session is supposed to be readonly, this will cause the right
    // AuthContext to constructed.
    readonly: bool,
}

impl SessionContainer {
    pub fn builder(fb: FacebookInit) -> SessionContainerBuilder {
        SessionContainerBuilder::new(fb)
    }

    pub fn new_with_defaults(fb: FacebookInit) -> Self {
        Self::builder(fb).build()
    }

    pub fn new_context(&self, logger: Logger, scuba: MononokeScubaSampleBuilder) -> CoreContext {
        let logging = LoggingContainer::new(self.fb, logger, scuba);

        CoreContext::new(self.fb, logging, self.clone())
    }

    pub fn new_context_with_scribe(
        &self,
        logger: Logger,
        scuba: MononokeScubaSampleBuilder,
        scribe: Scribe,
    ) -> CoreContext {
        let mut logging = LoggingContainer::new(self.fb, logger, scuba);
        logging.with_scribe(scribe);

        CoreContext::new(self.fb, logging, self.clone())
    }

    pub fn fb(&self) -> FacebookInit {
        self.fb
    }

    pub fn metadata(&self) -> &Metadata {
        &self.inner.metadata
    }

    pub fn rate_limiter(&self) -> Option<&(dyn RateLimiter + Send + Sync)> {
        match self.inner.rate_limiter {
            Some(ref rate_limiter) => Some(&**rate_limiter),
            None => None,
        }
    }

    pub fn bump_load(&self, metric: Metric, load: LoadCost) {
        if let Some(limiter) = self.rate_limiter() {
            limiter.bump_load(metric, load)
        }
    }

    pub fn check_load_shed(&self) -> Result<(), RateLimitReason> {
        match &self.inner.rate_limiter {
            Some(limiter) => limiter.check_load_shed(self.metadata().identities()),
            None => Ok(()),
        }
    }

    pub async fn check_rate_limit(&self, metric: Metric) -> Result<(), RateLimitReason> {
        match &self.inner.rate_limiter {
            Some(limiter) => limiter
                .check_rate_limit(metric, self.metadata().identities())
                .await
                .unwrap_or(Ok(())),
            None => Ok(()),
        }
    }

    pub fn is_quicksand(&self) -> bool {
        self.metadata().identities().is_quicksand()
    }

    pub fn is_readonly(&self) -> bool {
        self.inner.readonly
    }

    pub fn is_hg_sync_job(&self) -> bool {
        self.metadata().identities().is_hg_sync_job()
    }

    pub fn blobstore_read_limiter(&self) -> Option<&AsyncLimiter> {
        self.inner.blobstore_read_limiter.as_ref()
    }

    pub fn blobstore_write_limiter(&self) -> Option<&AsyncLimiter> {
        self.inner.blobstore_write_limiter.as_ref()
    }

    pub fn session_class(&self) -> SessionClass {
        self.session_class
    }

    pub fn override_session_class(&mut self, session_class: SessionClass) {
        self.session_class = session_class;
    }
}
