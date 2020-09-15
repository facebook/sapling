/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_limiter::AsyncLimiter;
use fbinit::FacebookInit;
use load_limiter::{BoxLoadLimiter, LoadCost, LoadLimiter, Metric};
use scribe_ext::Scribe;
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use sshrelay::Metadata;
use std::sync::Arc;
use std::time::Duration;
use tracing::TraceContext;

pub use self::builder::SessionContainerBuilder;
use crate::core::CoreContext;
use crate::logging::LoggingContainer;
use crate::{is_external_sync, is_quicksand};

mod builder;

#[derive(Clone)]
pub struct SessionContainer {
    fb: FacebookInit,
    inner: Arc<SessionContainerInner>,
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
}

struct SessionContainerInner {
    trace: TraceContext,
    metadata: Metadata,
    load_limiter: Option<BoxLoadLimiter>,
    blobstore_write_limiter: Option<AsyncLimiter>,
    blobstore_read_limiter: Option<AsyncLimiter>,
    session_class: SessionClass,
}

impl SessionContainer {
    pub fn builder(fb: FacebookInit) -> SessionContainerBuilder {
        SessionContainerBuilder::new(fb)
    }

    pub fn new_with_defaults(fb: FacebookInit) -> Self {
        Self::builder(fb).build()
    }

    pub fn new_context(&self, logger: Logger, scuba: ScubaSampleBuilder) -> CoreContext {
        let logging = LoggingContainer::new(self.fb, logger, scuba);

        CoreContext::new_with_containers(self.fb, logging, self.clone())
    }

    pub fn new_context_with_scribe(
        &self,
        logger: Logger,
        scuba: ScubaSampleBuilder,
        scribe: Scribe,
    ) -> CoreContext {
        let mut logging = LoggingContainer::new(self.fb, logger, scuba);
        logging.with_scribe(scribe);

        CoreContext::new_with_containers(self.fb, logging, self.clone())
    }

    pub fn fb(&self) -> FacebookInit {
        self.fb
    }

    pub fn trace(&self) -> &TraceContext {
        &self.inner.trace
    }

    pub fn metadata(&self) -> &Metadata {
        &self.inner.metadata
    }

    pub fn load_limiter(&self) -> Option<&dyn LoadLimiter> {
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

    pub async fn should_throttle(&self, metric: Metric, duration: Duration) -> Result<bool, !> {
        match &self.inner.load_limiter {
            Some(limiter) => match limiter.should_throttle(metric, duration).await {
                Ok(res) => Ok(res),
                Err(_) => Ok(false),
            },
            None => Ok(false),
        }
    }

    pub fn is_quicksand(&self) -> bool {
        is_quicksand(self.metadata())
    }

    pub fn is_external_sync(&self) -> bool {
        is_external_sync(self.metadata())
    }

    pub fn blobstore_read_limiter(&self) -> &Option<AsyncLimiter> {
        &self.inner.blobstore_read_limiter
    }

    pub fn blobstore_write_limiter(&self) -> &Option<AsyncLimiter> {
        &self.inner.blobstore_write_limiter
    }

    pub fn session_class(&self) -> SessionClass {
        self.inner.session_class
    }
}
