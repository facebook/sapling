/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_limiter::AsyncLimiter;
use fbinit::FacebookInit;
use load_limiter::{BoxLoadLimiter, LoadCost, LoadLimiter, Metric, ThrottleReason};
use permission_checker::MononokeIdentitySetExt;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use sshrelay::Metadata;
use std::sync::Arc;
use std::time::Duration;

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
    /// This session is used by the warm bookmarks cache.
    WarmBookmarksCache,
}

struct SessionContainerInner {
    metadata: Metadata,
    load_limiter: Option<BoxLoadLimiter>,
    blobstore_write_limiter: Option<AsyncLimiter>,
    blobstore_read_limiter: Option<AsyncLimiter>,
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

        CoreContext::new_with_containers(self.fb, logging, self.clone())
    }

    pub fn new_context_with_scribe(
        &self,
        logger: Logger,
        scuba: MononokeScubaSampleBuilder,
        scribe: Scribe,
    ) -> CoreContext {
        let mut logging = LoggingContainer::new(self.fb, logger, scuba);
        logging.with_scribe(scribe);

        CoreContext::new_with_containers(self.fb, logging, self.clone())
    }

    pub fn fb(&self) -> FacebookInit {
        self.fb
    }

    pub fn metadata(&self) -> &Metadata {
        &self.inner.metadata
    }

    pub fn load_limiter(&self) -> Option<&(dyn LoadLimiter + Send + Sync)> {
        match self.inner.load_limiter {
            Some(ref load_limiter) => Some(&**load_limiter),
            None => None,
        }
    }

    pub fn bump_load(&self, metric: Metric, load: LoadCost) {
        if let Some(limiter) = self.load_limiter() {
            limiter.bump_load(metric, load)
        }
    }

    pub async fn check_throttle(&self, metric: Metric) -> Result<(), ThrottleReason> {
        const LOAD_LIMIT_TIMEFRAME: Duration = Duration::from_secs(1);

        match &self.inner.load_limiter {
            Some(limiter) => limiter
                .check_throttle(metric, LOAD_LIMIT_TIMEFRAME)
                .await
                .unwrap_or(Ok(())),
            None => Ok(()),
        }
    }

    pub fn is_quicksand(&self) -> bool {
        self.metadata().identities().is_quicksand()
    }

    pub fn blobstore_read_limiter(&self) -> &Option<AsyncLimiter> {
        &self.inner.blobstore_read_limiter
    }

    pub fn blobstore_write_limiter(&self) -> &Option<AsyncLimiter> {
        &self.inner.blobstore_write_limiter
    }

    pub fn session_class(&self) -> SessionClass {
        self.session_class
    }

    pub fn override_session_class(&mut self, session_class: SessionClass) {
        self.session_class = session_class;
    }
}
